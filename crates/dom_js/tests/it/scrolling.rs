//! Scrolling from script, in a frame whose layout logs what it is asked
//! (no box: an element isn't a scroll container, the scrolling element
//! scrolls the viewport): the overloads (a dictionary, two numbers), the
//! dictionaries' members and their enumerations (a promise rejected with
//! a `TypeError`, WebIDL's for an operation giving one), the promises
//! resolved with undefined once scrolled; `scrollIntoView`'s argument.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// A frame's layout of a 100 by 100 viewport scrolled to (5, 6), logging
/// each scroll asked of it.
struct Logging(dom::NodeId, Rc<RefCell<Vec<String>>>);

impl dom_api::FrameLayout for Logging {
    fn shows(&mut self, document: dom::NodeId) -> bool {
        document == self.0
    }
    fn update_style_and_layout(&mut self, _: &dom::Cx<'_, '_>) {}
    fn element_box(&self, _: dom::NodeId) -> Option<gfx::ElementBox> {
        None
    }
    fn client_rects(&self, _: dom::NodeId) -> Vec<gfx::RectF> {
        Vec::new()
    }
    fn viewport(&self) -> gfx::Viewport {
        gfx::Viewport {
            size: gfx::SizeF::new(100.0, 100.0),
            device_scale_factor: 1.0,
            scroll: gfx::PointF::new(5.0, 6.0),
            content_size: gfx::SizeF::new(100.0, 1000.0),
        }
    }
    fn window_screen_rect(&self) -> Option<gfx::RectF> {
        None
    }
    fn computed_style<'gc>(
        &self,
        _: &dom::Cx<'_, 'gc>,
        _: dom::NodeRef<'gc>,
    ) -> Option<std::sync::Arc<computed_style::ComputedStyle>> {
        None
    }
    fn element_at(&mut self, _: gfx::PointF) -> Option<dom::NodeId> {
        None
    }
    fn elements_at(&mut self, _: gfx::PointF) -> Vec<dom::NodeId> {
        Vec::new()
    }
    fn scroll_to(&mut self, document: dom::NodeId, scroller: dom::NodeId, offset: gfx::PointF) {
        let what = if scroller == document {
            "viewport"
        } else {
            "element"
        };
        self.1
            .borrow_mut()
            .push(format!("{what} {} {}", offset.x, offset.y));
    }
    fn scroll_into_view<'gc>(
        &mut self,
        _: &dom::Cx<'_, 'gc>,
        _: dom::NodeRef<'gc>,
        params: gfx::ScrollIntoViewParams,
    ) {
        self.1.borrow_mut().push(format!(
            "into view {:?} {:?} {}",
            params.block, params.inline, params.nearest_only
        ));
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct Page {
    engine: V8Engine,
    log: Rc<RefCell<Vec<String>>>,
}

impl Page {
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let document = engine
            .create_window(engine.top_frame(), "https://example.test/", Instant::now())
            .expect("a window");
        let id = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
        drop(document);
        let log = Rc::new(RefCell::new(Vec::new()));
        engine.set_layout(Box::new(Logging(id, log.clone())));
        let mut page = Self { engine, log };
        page.eval("document.appendChild(document.createElement('html')); var e = document.documentElement; 0");
        page
    }

    /// `source`'s completion value as a string, or the name of what it
    /// throws, `!` first; then a microtask checkpoint.
    fn eval(&mut self, source: &str) -> String {
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

    /// What the promise `source` gives is rejected with: `!` and its name
    /// (WebIDL's: an operation giving a promise doesn't throw).
    fn rejected(&mut self, source: &str) -> String {
        // What a member returning a promise throws, before the operation
        // or in it, rejects it (`threw` if it doesn't).
        self.eval(&format!(
            "var rejected = 'none'; try {{ ({source}).then(null, (x) => {{ rejected = '!' + x.name; }}); }} \
             catch (x) {{ rejected = 'threw ' + x.name; }} 0"
        ));
        self.eval("rejected")
    }

    /// What `source` asked the layout to scroll.
    fn scrolls(&mut self, source: &str) -> Vec<String> {
        self.log.borrow_mut().clear();
        let result = self.eval(source);
        assert!(!result.starts_with('!'), "{source}: {result}");
        self.log.borrow().clone()
    }
}

#[test]
fn the_forms_and_their_members() {
    let mut page = Page::new();
    // The root element is the scrolling element: the viewport, from (5, 6).
    assert_eq!(
        page.scrolls("e.scrollTo({ top: 10 }); 0"),
        ["viewport 5 10"]
    );
    assert_eq!(page.scrolls("e.scroll(1, 2); 0"), ["viewport 1 2"]);
    assert_eq!(
        page.scrolls("e.scrollBy({ left: 1, behavior: 'smooth' }); 0"),
        ["viewport 6 6"]
    );
    assert_eq!(page.scrolls("scrollTo({ left: 3 }); 0"), ["viewport 3 6"]);
    assert_eq!(
        page.scrolls("scroll(0, 0); scrollBy(-1, 1); 0"),
        ["viewport 0 0", "viewport 4 7"]
    );
    assert_eq!(
        page.scrolls("e.scrollTop = 30; e.scrollLeft = Infinity; 0"),
        ["viewport 5 30", "viewport 0 6"]
    );
    // Not a scroll container (no box): nothing.
    assert!(page.scrolls("var d = e.appendChild(document.createElement('div')); d.scrollTo(1, 1); d.scrollTop = 4; 0").is_empty());
    assert_eq!(
        page.rejected("e.scrollTo({ behavior: 'fast' })"),
        "!TypeError"
    );
    assert_eq!(
        page.rejected("scrollBy({ behavior: 'fast' })"),
        "!TypeError"
    );
    // No form takes one argument that isn't a dictionary: rejected too,
    // as an argument that doesn't convert is.
    assert_eq!(
        page.rejected("e.scrollTo(1)"),
        "!TypeError",
        "not a dictionary"
    );
    // A promise resolved with undefined.
    assert_eq!(
        page.eval("var got = 'pending'; var p = e.scrollTo(0, 0); p.then((v) => { got = v; }); p instanceof Promise"),
        "true"
    );
    assert_eq!(page.eval("String(got)"), "undefined");
}

#[test]
fn scroll_into_view_arguments() {
    let mut page = Page::new();
    assert_eq!(
        page.scrolls(
            "e.scrollIntoView(); e.scrollIntoView(false); \
             e.scrollIntoView({ block: 'center', inline: 'end', container: 'nearest' }); e.scrollIntoView({}); 0"
        ),
        [
            "into view Start Nearest false",
            "into view End Nearest false",
            "into view Center End true",
            "into view Start Nearest false",
        ]
    );
    assert_eq!(
        page.rejected("e.scrollIntoView({ block: 'top' })"),
        "!TypeError"
    );
    assert_eq!(
        page.rejected("e.scrollIntoView({ container: 'none' })"),
        "!TypeError"
    );
    assert_eq!(
        page.rejected("e.scrollIntoView({ behavior: 'slow' })"),
        "!TypeError"
    );
}
