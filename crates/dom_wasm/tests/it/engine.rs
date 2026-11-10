//! wasmtime as a page's engine (`WasmEngine`): a window's document is the
//! engine's own DOM's, which a script's `start` gets and changes; a script
//! that waits is done for the page at its first wait, and goes on when the
//! engine's timers run past its deadline; one that traps is reported and
//! gone; what a guest prints goes to the console. A guest's listeners are
//! called through its `handle-event`, by the page's dispatch and by the
//! guest's own (a member that dispatches, within its call).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dom::{NodeRoot, QualName};
use dom_api::{ConsoleMessage, EngineSettings, ScriptEngine, WasmScript};
use dom_wasm::WasmEngine;

/// The page guest (`tests/guests/page.c`), read once.
fn guest() -> &'static [u8] {
    static PAGE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    PAGE.get_or_init(|| {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guests/page.wasm")).unwrap()
    })
}

/// An engine with a window, its document `<html data-case=case>`, the time
/// the test's, and the console's messages kept.
struct Page {
    // Before the engine: a root doesn't outlive its heap.
    document: NodeRoot,
    engine: WasmEngine,
    time: Arc<AtomicU64>,
    console: Rc<RefCell<Vec<ConsoleMessage>>>,
}

impl Page {
    fn new(case: &str) -> Self {
        let time = Arc::new(AtomicU64::new(1_000_000_000));
        let now = time.clone();
        let mut engine = WasmEngine::with_clock(
            EngineSettings { web_tests: false },
            Arc::new(move || now.load(Ordering::Relaxed)),
        );
        let console = Rc::new(RefCell::new(Vec::new()));
        let sink = console.clone();
        engine.set_console_sink(Box::new(move |message| sink.borrow_mut().push(message)));
        let top = engine.top_frame();
        let document = engine
            .create_window(top, "https://page.test/", Instant::now())
            .unwrap();
        engine.dom_mut().mutate(|cx| {
            let document = document.get(cx.gc);
            let html = cx.create_element(document, QualName::local("html"));
            cx.set_attribute(html, "data-case", case);
            cx.append_child(document, html).unwrap();
        });
        Self {
            engine,
            document,
            time,
            console,
        }
    }

    fn run(&mut self, bytes: &[u8]) {
        let top = self.engine.top_frame();
        self.engine.run_wasm_script(
            top,
            &WasmScript {
                bytes,
                url: "https://page.test/page.wasm",
            },
        );
    }

    /// The guests' clock moved on by `nanos`.
    fn advance(&self, nanos: u64) {
        let now = self.time.load(Ordering::Relaxed);
        self.time.store(now + nanos, Ordering::Relaxed);
    }

    /// The text of each child of the document element.
    fn said(&mut self) -> Vec<String> {
        let document = self.document.clone();
        self.engine.dom_mut().read(|cx| {
            let html = cx.first_child(document.get(cx.gc)).unwrap();
            let mut said = Vec::new();
            let mut child = cx.first_child(html);
            while let Some(at) = child {
                said.push(cx.text_content(at).unwrap().to_string());
                child = cx.next_sibling(at);
            }
            said
        })
    }

    /// `said`, without the elements that say nothing (the cases' inputs).
    fn lines(&mut self) -> Vec<String> {
        let mut said = self.said();
        said.retain(|line| !line.is_empty());
        said
    }

    /// A `click` event (a plain one: it doesn't bubble) fired by the page
    /// at the `nth` child of the document element.
    fn fire_click(&mut self, nth: usize) {
        let document = self.document.clone();
        let target = self.engine.dom_mut().read(|cx| {
            let html = cx.first_child(document.get(cx.gc)).unwrap();
            let mut child = cx.first_child(html).unwrap();
            for _ in 0..nth {
                child = cx.next_sibling(child).unwrap();
            }
            cx.root(child)
        });
        self.engine.fire_event(&target, dom::names::event::CLICK);
    }

    /// How many uncaught errors the console got.
    fn uncaught(&self) -> usize {
        self.console
            .borrow()
            .iter()
            .filter(|message| message.uncaught)
            .count()
    }

    fn console(&self) -> Vec<(String, bool)> {
        self.console
            .borrow()
            .iter()
            .map(|message| (message.text.clone(), message.uncaught))
            .collect()
    }
}

#[test]
fn a_script_starts_with_the_pages_document() {
    let mut page = Page::new("hello");
    assert_eq!(page.said(), [""; 0]);
    page.run(guest());
    assert_eq!(page.said(), ["hello from wasm"]);
    assert_eq!(page.console(), []);
    assert_eq!(page.engine.until_next_timer(), None);
    // Each script is its own store: again, on the same document.
    page.run(guest());
    assert_eq!(page.said(), ["hello from wasm", "hello from wasm"]);
    // The page's own work goes on with the DOM after: a full GC keeps it.
    page.engine.dom_mut().collect();
    assert_eq!(page.said(), ["hello from wasm", "hello from wasm"]);
}

#[test]
fn a_script_that_waits_goes_on_when_its_time_comes() {
    let mut page = Page::new("sleep");
    page.run(guest());
    // Done for the page at its first wait.
    assert_eq!(page.said(), ["before the wait"]);
    assert_eq!(
        page.engine.until_next_timer(),
        Some(Duration::from_millis(5))
    );
    // Not due yet: no turn.
    page.advance(4_999_999);
    assert_eq!(
        page.engine.until_next_timer(),
        Some(Duration::from_nanos(1))
    );
    assert!(!page.engine.run_due_tasks());
    assert!(!page.engine.run_timers());
    assert_eq!(page.said(), ["before the wait"]);
    page.advance(1);
    assert_eq!(page.engine.until_next_timer(), Some(Duration::ZERO));
    // The host's loop runs what is due as the DOM's tasks.
    assert!(page.engine.run_due_tasks());
    assert_eq!(page.said(), ["before the wait", "after the wait"]);
    assert_eq!(page.engine.until_next_timer(), None);
    assert!(!page.engine.run_timers());
}

#[test]
fn a_script_that_traps_is_reported_and_the_page_goes_on() {
    let mut page = Page::new("trap");
    page.run(guest());
    // What it did before stays.
    assert_eq!(page.said(), ["before the trap"]);
    let console = page.console();
    assert_eq!(console.len(), 1, "{console:?}");
    assert!(console[0].1, "uncaught");
    assert!(console[0].0.contains("unreachable"), "{console:?}");
    assert_eq!(page.console.borrow()[0].url, "https://page.test/page.wasm");
    // The next script runs.
    page.engine.dom_mut().mutate(|cx| {
        let html = cx.first_child(page.document.get(cx.gc)).unwrap();
        cx.set_attribute(html, "data-case", "hello");
    });
    page.run(guest());
    assert_eq!(page.said(), ["before the trap", "hello from wasm"]);
}

#[test]
fn bytes_that_are_no_component_are_reported() {
    let mut page = Page::new("hello");
    page.run(b"not wasm");
    let console = page.console();
    assert_eq!(console.len(), 1, "{console:?}");
    assert!(console[0].1);
    assert_eq!(page.said(), [""; 0]);
}

#[test]
fn what_a_guest_prints_goes_to_the_console() {
    let mut page = Page::new("print");
    page.run(guest());
    assert_eq!(page.console(), [("printed by the guest".to_owned(), false)]);
}

/// A new window's document is another, the scripts of the one before gone
/// (a sleeping one's wait with it).
#[test]
fn a_new_window_drops_the_scripts_of_the_one_before() {
    let mut page = Page::new("sleep");
    page.run(guest());
    assert!(page.engine.until_next_timer().is_some());
    let top = page.engine.top_frame();
    let next = page
        .engine
        .create_window(top, "https://page.test/next", Instant::now())
        .unwrap();
    assert_eq!(page.engine.until_next_timer(), None);
    page.advance(10_000_000);
    assert!(!page.engine.run_timers());
    let same = page
        .engine
        .dom_mut()
        .read(|cx| cx.node_id(next.get(cx.gc)) == cx.node_id(page.document.get(cx.gc)));
    assert!(!same);
    assert_eq!(page.said(), ["before the wait"]);
}

/// A guest that yields (its callback asks to be called again) goes on in
/// the same turn: nothing outside would run the store again for it.
#[test]
fn a_script_that_yields_goes_on_in_the_same_turn() {
    let mut page = Page::new("yield");
    page.run(guest());
    assert_eq!(
        page.said(),
        ["before the yields", "after one yield", "after two yields"]
    );
    assert_eq!(page.console(), []);
    assert_eq!(page.engine.until_next_timer(), None);
}

/// A guest reaches the page's window from its document (the same handle
/// each time, and back), and its location: the URL, and the fragment it
/// sets. A window is no node.
#[test]
fn a_guest_reaches_the_window_and_its_location() {
    let mut page = Page::new("window");
    page.run(guest());
    assert_eq!(
        page.said(),
        [
            "one window",
            "its document",
            "https://page.test/",
            "#/active",
            "a window is no node"
        ]
    );
    assert_eq!(page.console(), []);
}

/// `click()`, an element's own, which dispatches its event as it runs
/// (no listener hears it yet): a checkbox's default action checks it.
/// (`focus()` and `blur()` need the element rendered: `shell`'s
/// `wasm_documents` has them.)
#[test]
fn a_guest_clicks() {
    let mut page = Page::new("click");
    page.run(guest());
    assert_eq!(page.said(), ["", "checked"]);
    assert_eq!(page.console(), []);
}

/// What a listener says of a click made with `click()`.
const CLICKED: &str = "button 0 detail 1";

#[test]
fn the_pages_dispatch_calls_a_guests_listeners() {
    let mut page = Page::new("listen");
    page.run(guest());
    assert_eq!(page.lines(), [""; 0]);
    // The event doesn't bubble: the root's capturing listener, then the
    // target's (added twice, there once), and not the root's other.
    page.fire_click(0);
    assert_eq!(
        page.lines(),
        [
            "1:click of subject at root",
            "2:click of subject at subject"
        ]
    );
    assert_eq!(page.console(), []);
    // And again: the export is called each time.
    page.fire_click(0);
    assert_eq!(page.lines().len(), 4);
}

#[test]
fn each_listener_is_called_in_its_own_store() {
    let mut page = Page::new("listen");
    page.run(guest());
    page.run(guest());
    // The first script's checkbox: both scripts listen at the root (the
    // second's subject is its own), the first alone at the target.
    page.fire_click(0);
    assert_eq!(
        page.lines(),
        [
            "1:click of subject at root",
            "1:click at root",
            "2:click of subject at subject"
        ]
    );
}

#[test]
fn a_guests_own_dispatch_calls_its_listeners_before_it_returns() {
    let mut page = Page::new("listen-click");
    page.run(guest());
    assert_eq!(
        page.lines(),
        [
            format!("1:click of subject at root {CLICKED}"),
            format!("2:click of subject at subject {CLICKED}"),
            format!("3:click of subject at root {CLICKED}"),
            "clicked: checked".to_owned(),
        ]
    );
    assert_eq!(page.console(), []);
}

#[test]
fn a_guests_own_dispatch_calls_no_other_scripts_listener() {
    let mut page = Page::new("listen-click");
    page.run(guest());
    assert_eq!(page.lines().len(), 4);
    // The second script's click passes the first's listeners at the root:
    // they aren't called (the member's store alone is entered).
    page.run(guest());
    assert_eq!(
        page.lines()[4..],
        [
            format!("1:click of subject at root {CLICKED}"),
            format!("2:click of subject at subject {CLICKED}"),
            format!("3:click of subject at root {CLICKED}"),
            "clicked: checked".to_owned(),
        ]
    );
    assert_eq!(page.console(), []);
}

#[test]
fn a_listener_prevents_the_default_action() {
    let mut page = Page::new("prevent-click");
    page.run(guest());
    assert_eq!(
        page.lines(),
        [
            format!("18:click of subject at subject {CLICKED}"),
            "prevented".to_owned(),
            "clicked: not checked".to_owned(),
        ]
    );
}

#[test]
fn a_listener_stops_the_propagation() {
    let mut page = Page::new("stop-click");
    page.run(guest());
    assert_eq!(
        page.lines(),
        [
            format!("33:click of subject at root {CLICKED}"),
            "clicked: checked".to_owned(),
        ]
    );
}

#[test]
fn a_listener_is_removed_as_it_was_added() {
    let mut page = Page::new("remove-click");
    page.run(guest());
    // The capturing one was named with the other phase: it stays; so does
    // the target's other listener.
    assert_eq!(
        page.lines(),
        [
            format!("1:click of subject at root {CLICKED}"),
            format!("6:click of subject at subject {CLICKED}"),
            "clicked: checked".to_owned(),
        ]
    );
}

#[test]
fn a_listener_that_traps_ends_its_store() {
    let mut page = Page::new("listener-trap");
    page.run(guest());
    page.run(guest());
    assert_eq!(page.uncaught(), 0);
    page.fire_click(0);
    assert_eq!(page.lines(), ["66:click of subject at subject"]);
    assert_eq!(page.uncaught(), 1);
    assert_eq!(page.console.borrow()[0].url, "https://page.test/page.wasm");
    // Its listeners went with it; the other script's are still called.
    page.fire_click(0);
    assert_eq!(page.lines().len(), 1);
    page.fire_click(1);
    assert_eq!(
        page.lines(),
        [
            "66:click of subject at subject",
            "66:click of subject at subject"
        ]
    );
    assert_eq!(page.uncaught(), 2);
}

#[test]
fn a_listener_that_traps_in_the_guests_own_dispatch_ends_its_start() {
    let mut page = Page::new("trap-click");
    page.run(guest());
    // Neither the root's listener nor the rest of `start`.
    assert_eq!(
        page.lines(),
        [format!("66:click of subject at subject {CLICKED}")]
    );
    assert_eq!(page.uncaught(), 1);
    // The listener's own trap, not what calling the next one after it
    // would say.
    assert!(
        page.console()[0].0.contains("unreachable"),
        "{:?}",
        page.console()
    );
    page.fire_click(0);
    assert_eq!(page.lines().len(), 1);
    assert_eq!(page.uncaught(), 1);
}

#[test]
fn a_listener_dispatches_within_the_pages_dispatch() {
    let mut page = Page::new("nested");
    page.run(guest());
    page.fire_click(0);
    assert_eq!(
        page.lines(),
        [
            "130:click of subject at subject".to_owned(),
            format!("517:click elsewhere {CLICKED}"),
            "after the click within".to_owned(),
        ]
    );
    // What a listener prints is the console's when the dispatch is over.
    assert_eq!(
        page.console(),
        [("printed by a listener".to_owned(), false)]
    );
}

#[test]
fn an_events_handle_is_the_guests_for_the_call_only() {
    // The page's dispatch: the handle the listener kept is none of its
    // store's by its next call, and a member called with it traps.
    let mut page = Page::new("keep");
    page.run(guest());
    page.fire_click(0);
    assert_eq!(page.uncaught(), 0);
    page.fire_click(0);
    assert_eq!(
        page.lines(),
        [
            "258:click of subject at subject",
            "258:click of subject at subject"
        ]
    );
    assert_eq!(page.uncaught(), 1);
    assert!(page.console()[0].0.contains("is no handle of this store"));
}

#[test]
fn an_events_handle_in_the_guests_own_dispatch_is_its_for_the_call_only() {
    let mut page = Page::new("keep-click");
    page.run(guest());
    assert_eq!(
        page.lines(),
        [
            format!("258:click of subject at subject {CLICKED}"),
            format!("258:click of subject at subject {CLICKED}"),
        ]
    );
    assert_eq!(page.uncaught(), 1);
    assert!(page.console()[0].0.contains("is no handle of this store"));
}
