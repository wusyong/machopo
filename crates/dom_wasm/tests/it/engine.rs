//! wasmtime as a page's engine (`WasmEngine`): a window's document is the
//! engine's own DOM's, which a script's `start` gets and changes; a script
//! that waits is done for the page at its first wait, and goes on when the
//! engine's timers run past its deadline; one that traps is reported and
//! gone; what a guest prints goes to the console.

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
    assert_eq!(page.engine.until_next_timer(), Some(Duration::from_millis(5)));
    // Not due yet: no turn.
    page.time.fetch_add(4_999_999, Ordering::Relaxed);
    assert_eq!(page.engine.until_next_timer(), Some(Duration::from_nanos(1)));
    assert!(!page.engine.run_due_tasks());
    assert!(!page.engine.run_timers());
    assert_eq!(page.said(), ["before the wait"]);
    page.time.fetch_add(1, Ordering::Relaxed);
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
    page.time.fetch_add(10_000_000, Ordering::Relaxed);
    assert!(!page.engine.run_timers());
    let same = page.engine.dom_mut().read(|cx| {
        cx.node_id(next.get(cx.gc)) == cx.node_id(page.document.get(cx.gc))
    });
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
