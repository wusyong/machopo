//! `matchMedia()`, `MediaQueryList` and `MediaQueryListEvent` from script,
//! in a frame whose viewport the test sets; HTML's "evaluate media queries
//! and report changes" (`ScriptEngine::report_media_query_changes`) firing
//! `change`, oldest list first, at those whose result changed; and the
//! `CSS` namespace.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// A frame's layout of a viewport of the test's width, 1x.
struct Sized(dom::NodeId, Rc<Cell<f32>>);

impl dom_api::FrameLayout for Sized {
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
            size: gfx::SizeF::new(self.1.get(), 100.0),
            device_scale_factor: 1.0,
            scroll: gfx::PointF::new(0.0, 0.0),
            content_size: gfx::SizeF::new(self.1.get(), 100.0),
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
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct Page {
    engine: V8Engine,
    width: Rc<Cell<f32>>,
}

impl Page {
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let document = engine
            .create_window(engine.top_frame(), "https://example.test/", Instant::now())
            .expect("a window");
        let id = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
        drop(document);
        let width = Rc::new(Cell::new(500.0));
        engine.set_layout(Box::new(Sized(id, width.clone())));
        Self { engine, width }
    }

    /// `source`'s completion value as a string, or the name of what it
    /// throws, `!` first.
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
        self.engine.take_harness_results().expect("reported")
    }

    fn check(&mut self, cases: &[(&str, &str)]) {
        for (script, expected) in cases {
            assert_eq!(self.eval(script), *expected, "{script}");
        }
    }

    /// The viewport `width` wide, then the rendering update's step.
    fn resize(&mut self, width: f32) {
        self.width.set(width);
        self.engine
            .report_media_query_changes(self.engine.top_frame());
    }
}

#[test]
fn match_media_and_its_list() {
    let mut page = Page::new();
    page.check(&[
        (
            "var wide = matchMedia('(min-width: 400px)'); \
             [wide instanceof MediaQueryList, wide instanceof EventTarget, wide.media, wide.matches, \
              matchMedia('(min-width: 600px)').matches, wide === matchMedia('(min-width: 400px)'), \
              String(wide), matchMedia('NOT all').media].join()",
            "true,true,(min-width: 400px),true,false,false,[object MediaQueryList],not all",
        ),
        (
            "var e = new MediaQueryListEvent('change', { media: 'x', matches: true }); \
             [e instanceof Event, e.type, e.media, e.matches, new MediaQueryListEvent('a').media, e.isTrusted].join()",
            "true,change,x,true,,false",
        ),
    ]);
    // `matches` evaluates now, whatever the step saw.
    page.width.set(300.0);
    page.check(&[("wide.matches", "false")]);
}

/// `change` at each list whose result changed (oldest first, a
/// `MediaQueryListEvent` of its queries and result), through
/// `addListener`, `onchange` and `addEventListener` alike; none at one
/// whose didn't; none while the environment is the same.
#[test]
fn changes_are_reported() {
    let mut page = Page::new();
    page.check(&[(
        "var log = []; var a = matchMedia('(min-width: 400px)'); var b = matchMedia('(max-width: 450px)'); \
         var c = matchMedia('(min-width: 10px)'); \
         var fa = e => log.push('a ' + e.media + ' ' + e.matches + ' ' + (e instanceof MediaQueryListEvent) + ' ' + e.isTrusted); \
         a.addListener(fa); a.addListener(fa); \
         b.onchange = e => log.push('b ' + e.matches); c.addEventListener('change', () => log.push('c')); 0",
        "0",
    )]);
    page.resize(300.0);
    page.check(&[(
        "log.join(' | ')",
        "a (min-width: 400px) false true true | b true",
    )]);
    // The same environment: nothing.
    page.check(&[("log = []; 0", "0")]);
    page.resize(300.0);
    page.check(&[("log.length", "0")]);
    // Its listener taken off (added once, as the same callback is).
    page.check(&[("a.removeListener(fa); 0", "0")]);
    page.resize(500.0);
    page.check(&[("log.join(' | ')", "b false")]);
}

#[test]
fn the_css_namespace() {
    let mut page = Page::new();
    page.check(&[
        (
            "[typeof CSS, Object.prototype.toString.call(CSS), Object.getPrototypeOf(CSS) === Object.prototype, \
              Object.keys(CSS).sort().join(' ')].join()",
            "object,[object CSS],true,escape supports",
        ),
        ("new CSS()", "!TypeError"),
        ("CSS()", "!TypeError"),
        (
            "[CSS.supports('color', 'red'), CSS.supports('color', 'nope'), CSS.supports('(color: red)'), \
              CSS.supports('color: red') /* in parentheses, then */, CSS.supports('color', undefined), CSS.escape('1a b')].join()",
            "true,false,true,true,false,\\31 a\\ b",
        ),
        ("CSS.supports()", "!TypeError"),
    ]);
}
