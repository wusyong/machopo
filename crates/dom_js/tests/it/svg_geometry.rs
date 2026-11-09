//! SVG's geometry members from script: `SVGMatrix`, `SVGPoint` and
//! `SVGRect` (their operations, floats and doubles, a new object each
//! time), `createSVG*`; and through a frame's layout of the tests'
//! (`Laid`): `getBBox()`, `getCTM()`, `getScreenCTM()`, the client rects of
//! an SVG element, `SVGGeometryElement`'s and `SVGTextContentElement`'s
//! methods, an SVG element taking the focus, a `ResizeObserver`'s sizes of
//! one. The renderer lays it out.

use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;
use gfx::{Path, PointF, RectF, SvgGeometry, SvgTextChar, Transform};

/// A window whose document has `<html><body>` and an `<svg id=s>` with a
/// `<rect id=r>`, a `<path id=p>`, a `<text id=t>`, a `<line id=l>`, an
/// empty `<path id=q>` and a `<rect id=d>`; `log`, and `error(f)`, the
/// name of what `f` throws.
struct Page {
    document: dom::NodeRoot,
    engine: V8Engine,
}

impl Page {
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let document = engine
            .create_window(engine.top_frame(), "https://example.test/", Instant::now())
            .expect("a window");
        let mut page = Self { document, engine };
        page.run(
            "var log = [];
             document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
             document.body.innerHTML = '<svg id=s><rect id=r /><path id=p d=\"M0 0 L3 4\" /><text id=t>ab</text><line id=l /><path id=q /><rect id=d /></svg>';
             var $ = id => document.getElementById(id);
             var m = (a, b, c, d, e, f) => { const m = $('s').createSVGMatrix();
                 [m.a, m.b, m.c, m.d, m.e, m.f] = [a, b, c, d, e, f]; return m; };
             var values = m => [m.a, m.b, m.c, m.d, m.e, m.f].join(',');
             var error = f => {
                 try { f(); return 'no error'; } catch (e) {
                     return e instanceof DOMException ? e.name : e.constructor.name;
                 }
             };",
        );
        page
    }

    fn run(&mut self, source: &str) {
        self.engine.run_classic_script(
            self.engine.top_frame(),
            &ClassicScript {
                source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    }

    /// `log`, joined with ` / `.
    fn log(&mut self) -> String {
        self.run("testRunner.reportHarnessResults(log.join(' / '))");
        self.engine.take_harness_results().expect("reported")
    }

    /// The page laid out as `Laid` gives it.
    fn lay_out(&mut self) {
        let root = &self.document;
        let document = self.engine.dom_mut().read(|cx| cx.node_id(root.get(cx.gc)));
        self.engine.set_layout(Box::new(Laid { document }));
    }
}

#[test]
fn matrices_points_and_rects() {
    let mut page = Page::new();
    page.run(
        "var s = $('s');
         var i = s.createSVGMatrix(), p = s.createSVGPoint(), r = s.createSVGRect();
         log.push(String(i), values(i), `${p.x},${p.y}`, [r.x, r.y, r.width, r.height].join(','),
                  s.createSVGMatrix() !== i);
         var a = m(1, 2, 3, 4, 5, 6);
         log.push(values(a.multiply(m(2, 0, 0, 2, 1, 1))), values(a), values(a.inverse()));
         log.push(values(a.translate(1, 2)), values(a.scale(2)), values(a.scaleNonUniform(2, 3)),
                  values(m(1, 0, 0, 1, 0, 0).flipX()), values(m(1, 0, 0, 1, 0, 0).flipY()));
         log.push(values(m(1, 0, 0, 1, 0, 0).rotate(90)).split(',').map(Math.round).join(','),
                  values(m(1, 0, 0, 1, 0, 0).rotateFromVector(1, 1)).split(',').map(v => Math.round(v * 100)).join(','),
                  values(m(1, 0, 0, 1, 0, 0).skewX(45)).split(',').map(Math.round).join(','),
                  values(m(1, 0, 0, 1, 0, 0).skewY(45)).split(',').map(Math.round).join(','));
         // The operations' arguments are floats; the attributes doubles.
         log.push(m(1, 0, 0, 1, 0, 0).translate(0.1, 0).e, (i.a = 0.1, i.a));
         log.push(error(() => m(1, 2, 2, 4, 0, 0).inverse()), error(() => i.rotateFromVector(0, 1)),
                  error(() => i.translate(NaN, 0)), error(() => { i.a = Infinity; }),
                  error(() => i.multiply({})));
         p.x = 0.1; p.y = 2;
         var moved = p.matrixTransform(m(1, 0, 0, 1, 10, 20));
         log.push(p.x, `${moved.x},${moved.y}`, moved !== p, String(moved));
         r.width = 0.1; r.height = -3;
         log.push(r.width, r.height, String(r), error(() => { r.x = NaN; }));",
    );
    assert_eq!(
        page.log(),
        "[object SVGMatrix] / 1,0,0,1,0,0 / 0,0 / 0,0,0,0 / true / \
         2,4,6,8,9,12 / 1,2,3,4,5,6 / -2,1,1.5,-0.5,1,-2 / \
         1,2,3,4,12,16 / 2,4,6,8,5,6 / 2,4,9,12,5,6 / -1,0,0,1,0,0 / 1,0,0,-1,0,0 / \
         0,1,-1,0,0,0 / 71,71,-71,71,0,0 / 1,0,1,1,0,0 / 1,1,0,1,0,0 / \
         0.10000000149011612 / 0.1 / \
         InvalidStateError / InvalidAccessError / TypeError / TypeError / TypeError / \
         0.10000000149011612 / 10.100000381469727,22 / true / [object SVGPoint] / \
         0.10000000149011612 / -3 / [object SVGRect] / TypeError"
    );
}

/// With no frame's layout: the identity and nothing laid out; a `<path>`
/// measured along its `d` all the same (Blink's reads its path data), a
/// `<rect>` no shape.
#[test]
fn with_no_layout() {
    let mut page = Page::new();
    page.run(
        "var b = $('r').getBBox();
         log.push(String(b), [b.x, b.y, b.width, b.height].join(','), values($('r').getCTM()),
                  values($('r').getScreenCTM()), $('r').getBoundingClientRect().width,
                  $('p').getTotalLength(), error(() => $('r').getTotalLength()),
                  $('p').isPointInFill({x: 1, y: 1}),
                  $('t').getNumberOfChars(), error(() => $('t').getStartPositionOfChar(0)));",
    );
    assert_eq!(
        page.log(),
        "[object SVGRect] / 0,0,0,0 / 1,0,0,1,0,0 / 1,0,0,1,0,0 / 0 / 5 / InvalidStateError / \
         false / 0 / IndexSizeError"
    );
}

#[test]
fn through_the_layout() {
    let mut page = Page::new();
    page.lay_out();
    page.run(
        "var r = $('r'), p = $('p'), t = $('t');
         var b = r.getBBox(), rect = r.getBoundingClientRect();
         log.push([b.x, b.y, b.width, b.height].join(','), values(r.getCTM()), values(r.getScreenCTM()),
                  [rect.x, rect.y, rect.width, rect.height].join(','), r.getClientRects().length);
         log.push(p.getTotalLength(), [p.getPointAtLength(2.5).x, p.getPointAtLength(2.5).y].join(','),
                  p.getPointAtLength(-1).x, p.getPointAtLength(9).y,
                  [$('l').getPointAtLength(9).x, $('l').getPointAtLength(9).y].join(','),
                  $('q').getTotalLength(), error(() => $('q').getPointAtLength(1)));
         log.push(r.isPointInFill({x: 1, y: 2}), r.isPointInFill({x: 2, y: 2}), r.isPointInStroke({x: 5, y: 2}),
                  r.isPointInFill({x: Infinity, y: 2}), r.isPointInStroke({x: 1, y: NaN}), r.isPointInFill());
         // Two characters, the first two code units (an emoji's).
         log.push(t.getNumberOfChars(), t.getComputedTextLength(), t.getSubStringLength(1, 1),
                  t.getSubStringLength(2, 5), t.getSubStringLength(0, 1),
                  error(() => t.getSubStringLength(3, 1)));
         var start = t.getStartPositionOfChar(2), end = t.getEndPositionOfChar(1), e = t.getExtentOfChar(2);
         log.push(`${start.x},${start.y}`, `${end.x},${end.y}`, [e.x, e.y, e.width, e.height].join(','),
                  t.getRotationOfChar(2), t.getCharNumAtPosition({x: 15, y: 0}),
                  t.getCharNumAtPosition({x: 50, y: 0}), error(() => t.getRotationOfChar(3)),
                  error(() => t.selectSubString(3, 1)), error(() => t.selectSubString(2, 9)));",
    );
    assert_eq!(
        page.log(),
        "1,2,3,4 / 2,0,0,2,0,0 / 2,0,0,2,10,20 / 12,24,6,8 / 1 / \
         5 / 1.5,2 / 0 / 4 / 3,4 / 0 / InvalidStateError / \
         true / false / true / false / false / true / \
         3 / 20 / 12 / 8 / 12 / IndexSizeError / \
         10,0 / 12,0 / 10,-5,8,7 / 30 / 2 / -1 / IndexSizeError / IndexSizeError / no error"
    );
}

#[test]
fn an_svg_element_takes_the_focus_when_rendered() {
    let mut page = Page::new();
    page.lay_out();
    page.run(
        "var r = $('r'), p = $('p');
         r.tabIndex = 0; p.tabIndex = 0;
         r.focus(); log.push(document.activeElement === r);
         p.focus(); log.push(document.activeElement === r);",
    );
    assert_eq!(page.log(), "true / true");
}

/// An SVG graphics element listening for focus events takes the focus
/// (its `tabIndex` still -1), not for another event's listener; another
/// SVG element's listener doesn't make it focusable (a `<stop>`, rendered
/// here as `r` is, which a `tabindex` does). As Chrome 152 does.
#[test]
fn an_svg_graphics_element_takes_the_focus_for_its_listener() {
    let mut page = Page::new();
    page.lay_out();
    page.run(
        "var r = $('r');
         r.addEventListener('click', () => {});
         r.focus(); log.push(document.activeElement === document.body);
         r.addEventListener('focusout', () => {});
         r.focus(); log.push(document.activeElement === r, r.tabIndex);
         r.blur(); r.id = '';
         var stop = $('s').appendChild(
             document.createElementNS('http://www.w3.org/2000/svg', 'stop'));
         stop.id = 'r';
         stop.addEventListener('focus', () => {});
         stop.focus(); log.push(document.activeElement === document.body);
         stop.tabIndex = 0; stop.focus(); log.push(document.activeElement === stop);",
    );
    assert_eq!(page.log(), "true / true / -1 / true / true");
}

#[test]
fn a_resize_observer_sees_an_svg_elements_bounding_box() {
    let mut page = Page::new();
    page.lay_out();
    page.run(
        "var observer = new ResizeObserver(entries => {
             for (const e of entries) {
                 log.push(e.target.id, [e.contentRect.x, e.contentRect.y, e.contentRect.width,
                     e.contentRect.height].join(','), e.borderBoxSize[0].inlineSize,
                     e.devicePixelContentBoxSize[0].blockSize);
             }
         });
         observer.observe($('r')); observer.observe($('d'));",
    );
    page.engine
        .gather_and_broadcast_resizes(page.engine.top_frame(), 0);
    // `#d` isn't rendered: nothing, its bounding box though it has one.
    assert_eq!(page.log(), "r / 0,0,3,4 / 3 / 4 / d / 0,0,0,0 / 0 / 0");
}

/// A frame's layout as the tests give it, by the elements' ids: `#r`'s
/// box (1, 2, 3 by 4) scaled by 2, at (10, 20) on the screen, its fill
/// left of x = 2 and its stroke anywhere; `#d` not rendered, its box 5 by
/// 5 all the same (one in `<defs>`); `#p`'s and `#l`'s path from (0, 0) to
/// (3, 4), `#q`'s empty; `#t`'s characters: an emoji of two code units 12 across, then
/// one 8 across at (10, 0), turned 30 degrees; nothing else rendered.
struct Laid {
    document: dom::NodeId,
}

fn id_of<'gc>(cx: &dom::Cx<'_, 'gc>, element: dom::NodeRef<'gc>) -> String {
    cx.get_attribute(element, "id")
        .map(|id| id.to_string())
        .unwrap_or_default()
}

impl dom_api::FrameLayout for Laid {
    fn shows(&mut self, document: dom::NodeId) -> bool {
        document == self.document
    }
    fn update_style_and_layout(&mut self, _: &dom::Cx<'_, '_>) {}
    fn element_box(&self, _: dom::NodeId) -> Option<gfx::ElementBox> {
        None
    }
    fn client_rects(&self, _: dom::NodeId) -> Vec<RectF> {
        Vec::new()
    }
    fn viewport(&self) -> gfx::Viewport {
        gfx::Viewport {
            size: gfx::SizeF::new(800.0, 600.0),
            device_scale_factor: 1.0,
            scroll: PointF::new(0.0, 0.0),
            content_size: gfx::SizeF::new(800.0, 600.0),
        }
    }
    fn window_screen_rect(&self) -> Option<RectF> {
        None
    }
    fn computed_style<'gc>(
        &self,
        _: &dom::Cx<'_, 'gc>,
        _: dom::NodeRef<'gc>,
    ) -> Option<std::sync::Arc<computed_style::ComputedStyle>> {
        None
    }
    fn element_at(&mut self, _: PointF) -> Option<dom::NodeId> {
        None
    }
    fn elements_at(&mut self, _: PointF) -> Vec<dom::NodeId> {
        Vec::new()
    }
    fn svg_geometry<'gc>(&self, cx: &dom::Cx<'_, 'gc>, element: dom::NodeRef<'gc>) -> SvgGeometry {
        match id_of(cx, element).as_str() {
            "r" => {}
            "d" => {
                return SvgGeometry {
                    bbox: RectF::new(0.0, 0.0, 5.0, 5.0),
                    ..SvgGeometry::default()
                };
            }
            _ => return SvgGeometry::default(),
        }
        let bbox = RectF::new(1.0, 2.0, 3.0, 4.0);
        let screen_ctm = Transform::translate(10.0, 20.0).then(&Transform::scale(2.0, 2.0));
        SvgGeometry {
            bbox,
            ctm: Transform::scale(2.0, 2.0),
            screen_ctm,
            client_rect: Some(screen_ctm.map_rect(bbox)),
        }
    }
    fn svg_shape_path<'gc>(
        &self,
        cx: &dom::Cx<'_, 'gc>,
        element: dom::NodeRef<'gc>,
    ) -> Option<Path> {
        let mut path = Path::new();
        match id_of(cx, element).as_str() {
            "p" | "l" => {
                path.move_to(PointF::new(0.0, 0.0));
                path.line_to(PointF::new(3.0, 4.0));
            }
            "q" => {}
            _ => return None,
        }
        Some(path)
    }
    fn svg_point_in_shape<'gc>(
        &self,
        cx: &dom::Cx<'_, 'gc>,
        element: dom::NodeRef<'gc>,
        point: PointF,
        stroke: bool,
    ) -> bool {
        id_of(cx, element) == "r" && (stroke || point.x < 2.0)
    }
    fn svg_text_chars<'gc>(
        &self,
        cx: &dom::Cx<'_, 'gc>,
        element: dom::NodeRef<'gc>,
    ) -> Vec<SvgTextChar> {
        if id_of(cx, element) != "t" {
            return Vec::new();
        }
        let cell = |units, x: f32, advance: f32, rotation| SvgTextChar {
            units,
            start: PointF::new(x, 0.0),
            end: PointF::new(x + advance, 0.0),
            corners: [
                PointF::new(x, -5.0),
                PointF::new(x + advance, -5.0),
                PointF::new(x + advance, 2.0),
                PointF::new(x, 2.0),
            ],
            rotation,
            advance,
        };
        vec![cell(2, 0.0, 12.0, 0.0), cell(1, 10.0, 8.0, 30.0)]
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
