//! `<canvas>` and its 2D context from script: `getContext`, the members'
//! arguments and what they leave in the context's data (its state, the
//! ops recorded, the path), and the canvases that drew. The renderer
//! draws the ops (`shell`'s tests check the pixels).

use std::time::Instant;

use canvas::CanvasData;
use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// A window whose document has `<html><body><canvas id=c>` and its 2D
/// context `ctx`; `log`, and `error(f)`, the name of what `f` throws.
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
             var c = document.createElement('canvas');
             c.setAttribute('id', 'c');
             document.body.appendChild(c);
             var ctx = c.getContext('2d');
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

    /// `f` on the data of `#c`'s context.
    fn data<R>(&mut self, f: impl FnOnce(&mut CanvasData) -> R) -> R {
        let document = &self.document;
        self.engine.dom_mut().read(|cx| {
            let canvas = cx
                .element_by_id(document.get(cx.gc), "c")
                .expect("the canvas");
            let context = cx.canvas_context(canvas).expect("its context");
            cx.with_canvas_data(context, f)
        })
    }

    /// The ids of the canvases that drew since last asked.
    fn draws(&mut self) -> Vec<String> {
        self.engine.dom_mut().read(|cx| {
            cx.take_canvas_draws()
                .into_iter()
                .map(|id| {
                    let node = cx.node_by_id(id).expect("a canvas");
                    cx.get_attribute(node, "id")
                        .map(|id| id.to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
    }
}

/// The interfaces, `width` and `height` (300 and 150 by default, else
/// the attributes), and `getContext`: the same 2D context each time (the
/// options of the first), `null` for other ids, the options' enumerations
/// checked whatever the id.
#[test]
fn get_context() {
    let mut page = Page::new();
    page.run(
        "log.push(typeof HTMLCanvasElement, typeof CanvasRenderingContext2D,
                  c instanceof HTMLCanvasElement, String(ctx), ctx.canvas === c);
         log.push(error(() => new CanvasRenderingContext2D()));
         log.push(`${c.width}x${c.height}`);
         c.width = 20; c.setAttribute('height', '10');
         log.push(`${c.width}x${c.height} ${c.getAttribute('width')}`);
         c.setAttribute('width', '-1');
         log.push(c.width);
         log.push(c.getContext('2d') === ctx, c.getContext('2d', {alpha: false}) === ctx);
         log.push(...[c.getContext('webgl'), c.getContext('bitmaprenderer'), c.getContext('2D')].map(String));
         log.push(error(() => c.getContext()));
         log.push(error(() => c.getContext('2d', {colorSpace: 'rec2020'})));
         log.push(error(() => c.getContext('webgl', {colorType: 'float32'})));
         var other = document.createElement('canvas');
         var opaque = other.getContext('2d', {alpha: false, colorSpace: 'display-p3',
                                              colorType: 'float16', willReadFrequently: true});
         log.push(opaque !== ctx, opaque.canvas === other, ctx.isContextLost());",
    );
    assert_eq!(
        page.log(),
        "function / function / true / [object CanvasRenderingContext2D] / true / TypeError / \
         300x150 / 20x10 20 / 300 / true / true / null / null / null / TypeError / TypeError / \
         TypeError / true / true / false"
    );
    let settings = page.engine.dom_mut().read(|cx| {
        let canvas = cx
            .element_by_id(page.document.get(cx.gc), "c")
            .expect("the canvas");
        cx.canvas_context_settings(cx.canvas_context(canvas).expect("its context"))
    });
    assert!(settings.alpha, "the first call's options");
}

/// The state's attributes read back what was set, ignore what isn't
/// valid (not finite, not positive, not one of the enumeration's values,
/// not a color), and `save()` and `restore()` keep and bring them back;
/// `restore()` with nothing saved does nothing.
#[test]
fn the_state() {
    let mut page = Page::new();
    page.run(
        "const state = () => [ctx.globalAlpha, ctx.globalCompositeOperation, ctx.fillStyle,
             ctx.strokeStyle, ctx.lineWidth, ctx.lineCap, ctx.lineJoin, ctx.miterLimit,
             ctx.getLineDash().join(','), ctx.lineDashOffset].join(' ');
         log.push(state());
         ctx.globalAlpha = 0.5; ctx.globalCompositeOperation = 'multiply';
         ctx.fillStyle = 'rgb(255 0 0 / 50%)'; ctx.strokeStyle = 'blue';
         ctx.lineWidth = 3; ctx.lineCap = 'round'; ctx.lineJoin = 'bevel'; ctx.miterLimit = 4;
         ctx.setLineDash([1, 2, 3]); ctx.lineDashOffset = 2;
         log.push(state());
         ctx.save();
         ctx.globalAlpha = 2; ctx.globalAlpha = NaN; ctx.globalCompositeOperation = 'nope';
         ctx.fillStyle = 'not a color'; ctx.strokeStyle = '';
         ctx.lineWidth = 0; ctx.lineWidth = -1; ctx.lineWidth = Infinity;
         ctx.lineCap = 'SQUARE'; ctx.lineJoin = 'mitre'; ctx.miterLimit = NaN;
         ctx.setLineDash([1, -1]); ctx.setLineDash([1, NaN]); ctx.lineDashOffset = Infinity;
         log.push(state());
         ctx.globalAlpha = 1; ctx.fillStyle = '#00ff00'; ctx.lineCap = 'square';
         ctx.setLineDash([]);
         log.push(state());
         ctx.restore();
         log.push(state());
         ctx.restore(); ctx.restore();
         log.push(state());
         log.push(error(() => ctx.setLineDash(5)));",
    );
    assert_eq!(
        page.log(),
        "1 source-over #000000 #000000 1 butt miter 10  0 / \
         0.5 multiply rgba(255, 0, 0, 0.5) #0000ff 3 round bevel 4 1,2,3,1,2,3 2 / \
         0.5 multiply rgba(255, 0, 0, 0.5) #0000ff 3 round bevel 4 1,2,3,1,2,3 2 / \
         1 multiply #00ff00 #0000ff 3 square bevel 4  2 / \
         0.5 multiply rgba(255, 0, 0, 0.5) #0000ff 3 round bevel 4 1,2,3,1,2,3 2 / \
         0.5 multiply rgba(255, 0, 0, 0.5) #0000ff 3 round bevel 4 1,2,3,1,2,3 2 / \
         TypeError"
    );
    assert_eq!(page.data(|data| data.level()), 0, "restored to the first");
}

/// `currentcolor` is the canvas's inline `color` as it is set (black
/// without one, or disconnected); a later change doesn't change it.
#[test]
fn current_color() {
    let mut page = Page::new();
    page.run(
        "ctx.fillStyle = 'currentcolor';
         log.push(ctx.fillStyle);
         c.style.color = 'lime';
         ctx.fillStyle = 'currentColor';
         log.push(ctx.fillStyle);
         c.style.color = 'red';
         log.push(ctx.fillStyle);
         c.remove();
         ctx.strokeStyle = 'currentcolor';
         log.push(ctx.strokeStyle);",
    );
    assert_eq!(page.log(), "#000000 / #00ff00 / #00ff00 / #000000");
}

/// The transforms compose with the current one in CSS's order;
/// `setTransform` and `resetTransform` replace it; what isn't finite is
/// ignored. `reset()` brings the default state back, the stack emptied,
/// and clears the bitmap: the canvas drew.
#[test]
fn transforms_and_reset() {
    let mut page = Page::new();
    assert_eq!(page.data(|data| data.size), (300, 150), "the default size");
    page.run("ctx.translate(10, 20); ctx.scale(2, 3);");
    assert_eq!(
        page.data(|data| data.state().transform),
        [2.0, 0.0, 0.0, 3.0, 10.0, 20.0]
    );
    page.run("ctx.transform(1, 0, 0, 1, 1, 1); ctx.scale(NaN, 1); ctx.translate(Infinity, 0);");
    assert_eq!(
        page.data(|data| data.state().transform),
        [2.0, 0.0, 0.0, 3.0, 12.0, 23.0]
    );
    page.run("ctx.setTransform(1, 2, 3, 4, 5, 6); ctx.setTransform(1, 0, 0, 1, 0, NaN);");
    assert_eq!(
        page.data(|data| data.state().transform),
        [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    );
    page.run("ctx.resetTransform(); ctx.rotate(Math.PI / 2);");
    let [a, b, c, d, e, f] = page.data(|data| data.state().transform);
    assert!(
        a.abs() < 1e-12 && (b - 1.0).abs() < 1e-12 && (c + 1.0).abs() < 1e-12 && d.abs() < 1e-12
    );
    assert_eq!((e, f), (0.0, 0.0));
    page.draws();
    page.run("ctx.save(); ctx.lineWidth = 5; ctx.moveTo(1, 1); ctx.reset();");
    assert_eq!(page.draws(), ["c"], "cleared");
    let (level, transform, width, empty) = page.data(|data| {
        (
            data.level(),
            data.state().transform,
            data.state().line_width,
            data.path.is_empty(),
        )
    });
    assert_eq!(
        (level, transform, width, empty),
        (0, canvas::context::IDENTITY, 1.0, true)
    );
}

/// What draws records ops (the transform first) and the canvas as one
/// that drew (once however many times); an empty or not finite rect
/// records nothing; `clearRect` does too.
#[test]
fn draws_record_ops() {
    let mut page = Page::new();
    assert_eq!(
        page.draws(),
        Vec::<String>::new(),
        "getContext draws nothing"
    );
    page.run("ctx.fillRect(0, 0, 10, 10); ctx.strokeRect(1, 1, 5, 5); ctx.clearRect(0, 0, 2, 2);");
    assert_eq!(page.data(|data| data.ops.len()), 4);
    assert_eq!(page.draws(), ["c"]);
    page.run(
        "ctx.fillRect(0, 0, 0, 10); ctx.fillRect(NaN, 0, 1, 1); ctx.strokeRect(0, 0, Infinity, 1);",
    );
    assert_eq!(page.data(|data| data.ops.len()), 4);
    assert_eq!(page.draws(), Vec::<String>::new());
    page.run(
        "ctx.beginPath(); ctx.rect(0, 0, 4, 4); ctx.fill(); ctx.stroke(); ctx.fill('evenodd');",
    );
    assert_eq!(page.data(|data| data.ops.len()), 7);
    assert_eq!(page.draws(), ["c"]);
}

/// The path: each method adds to it (`beginPath` empties it); what isn't
/// finite is ignored; a negative radius is an `IndexSizeError`; a fill
/// rule not one of `CanvasFillRule`'s a `TypeError`. `clip` keeps the
/// clip with the state.
#[test]
fn paths() {
    let mut page = Page::new();
    page.run(
        "ctx.moveTo(1, 1); ctx.lineTo(5, 1); ctx.quadraticCurveTo(6, 2, 5, 3);
         ctx.bezierCurveTo(4, 4, 3, 4, 2, 3); ctx.arcTo(1, 3, 1, 2, 1);
         ctx.arc(3, 3, 1, 0, Math.PI, true); ctx.ellipse(3, 3, 2, 1, 0, 0, 2 * Math.PI);
         ctx.rect(0, 0, 1, 1); ctx.closePath();
         ctx.lineTo(NaN, 1);
         log.push(error(() => ctx.arc(0, 0, -1, 0, 1)));
         log.push(error(() => ctx.arcTo(0, 0, 1, 1, -1)));
         log.push(error(() => ctx.ellipse(0, 0, 1, -1, 0, 0, 1)));
         log.push(error(() => ctx.fill('even-odd')));
         log.push(error(() => ctx.clip('nonzero ')));
         log.push(error(() => ctx.fill(undefined)));",
    );
    assert_eq!(
        page.log(),
        "IndexSizeError / IndexSizeError / IndexSizeError / TypeError / TypeError / no error"
    );
    assert!(!page.data(|data| data.path.is_empty()));
    page.run("ctx.save(); ctx.clip(); ctx.beginPath();");
    assert_eq!(
        page.data(|data| (data.state().clips.len(), data.path.is_empty())),
        (1, true)
    );
    page.run("ctx.restore();");
    assert_eq!(page.data(|data| data.state().clips.len()), 0);
}

/// Setting `width` or `height`, even to the same value, resets the
/// context to its default state, at the bitmap's new size, and the canvas
/// draws (cleared).
#[test]
fn a_size_change_resets_the_context() {
    let mut page = Page::new();
    page.run("ctx.fillStyle = 'red'; ctx.translate(1, 1); ctx.save(); ctx.fillRect(0, 0, 1, 1);");
    page.draws();
    page.run("c.width = c.width; log.push(ctx.fillStyle);");
    assert_eq!(page.log(), "#000000");
    let (level, transform, ops) =
        page.data(|data| (data.level(), data.state().transform, data.ops.len()));
    assert_eq!((level, transform, ops), (0, canvas::context::IDENTITY, 0));
    assert_eq!(page.draws(), ["c"]);
    page.run("c.height = 40;");
    assert_eq!(page.data(|data| data.size), (300, 40));
}

/// The frame's layout as the tests give it: it draws a canvas when asked
/// (its recording let go of, as the renderer's drawing it does), and
/// counts that.
struct Drawing {
    document: dom::NodeId,
    flushes: Vec<(dom::NodeId, canvas::CanvasFlush)>,
}

impl dom_api::FrameLayout for Drawing {
    fn shows(&mut self, document: dom::NodeId) -> bool {
        document == self.document
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
            size: gfx::SizeF::new(0.0, 0.0),
            device_scale_factor: 1.0,
            scroll: gfx::PointF::new(0.0, 0.0),
            content_size: gfx::SizeF::new(0.0, 0.0),
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
    fn flush_canvas(
        &mut self,
        cx: &dom::Cx<'_, '_>,
        canvas: dom::NodeId,
        flush: canvas::CanvasFlush,
    ) {
        let context = cx
            .canvas_context(cx.node_by_id(canvas).expect("a canvas"))
            .expect("its context");
        cx.with_canvas_data(context, |data| {
            data.ops.clear();
            data.matrix = None;
        });
        self.flushes.push((canvas, flush));
    }
    /// Each character a glyph 10px across, of a font 8px up and 2px down
    /// (its 1em box 7 and 3) with no BASE table; its ink each glyph's
    /// box, 1 less on each side.
    fn shape_canvas_text(
        &mut self,
        text: &str,
        style: &computed_style::CanvasTextStyle,
        ink: bool,
    ) -> Option<gfx::ShapedText> {
        let count = text.chars().count();
        let width = 10.0 * count as f32;
        let font = gfx::FontData {
            bytes: std::sync::Arc::new(Vec::<u8>::new()),
            index: 0,
            id: 1,
        };
        Some(gfx::ShapedText {
            runs: vec![gfx::GlyphRun {
                font,
                size: style.font.size,
                glyphs: vec![1; count],
                positions: (0..count)
                    .map(|i| gfx::PointF::new(10.0 * i as f32, 0.0))
                    .collect(),
                embolden: false,
                skew: 0.0,
                variations: std::sync::Arc::new([]),
            }],
            width,
            metrics: gfx::FontMetrics {
                ascent: 8.0,
                descent: 2.0,
                typo_ascent: 7.0,
                typo_descent: 3.0,
                ..Default::default()
            },
            ink: if ink {
                gfx::RectF::new(1.0, -7.0, width - 2.0, 8.0)
            } else {
                gfx::RectF::new(0.0, 0.0, 0.0, 0.0)
            },
        })
    }
    /// A file that says what it was made of.
    fn encode_image(
        &mut self,
        bitmap: &canvas::export::ExportBitmap,
        encoding: canvas::export::ImageEncoding,
    ) -> Option<Vec<u8>> {
        assert!(bitmap.pixels.wait_ready(), "pixels of the main thread's");
        let (width, height) = bitmap.size;
        let len = bitmap.pixels.as_bytes().len();
        Some(format!("{encoding:?} {width}x{height} {} {len}", bitmap.opaque).into_bytes())
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// `page` with the tests' layout.
fn with_drawing(page: &mut Page) {
    let document = page
        .engine
        .dom_mut()
        .read(|cx| cx.node_id(page.document.get(cx.gc)));
    page.engine.set_layout(Box::new(Drawing {
        document,
        flushes: Vec::new(),
    }));
}

/// A recording of `MAX_RECORDED_OPS` ops is drawn at once, not at the
/// next frame (Blink's `FlushIfRecordingLimitExceeded`): one draw for
/// 10 001 `fillRect`s (and their transform), the rest recorded after it;
/// a canvas in no document too (the frame's renderer draws any canvas).
#[test]
fn too_much_recorded_is_drawn_at_once() {
    let mut page = Page::new();
    let (document, canvas) = page.engine.dom_mut().read(|cx| {
        let document = page.document.get(cx.gc);
        let canvas = cx.element_by_id(document, "c").expect("the canvas");
        (cx.node_id(document), cx.node_id(canvas))
    });
    page.engine.set_layout(Box::new(Drawing {
        document,
        flushes: Vec::new(),
    }));
    page.run(
        "for (let i = 0; i < 10001; i++) ctx.fillRect(0, 0, 1, 1);
         var detached = document.createElement('canvas');
         detached.setAttribute('id', 'detached');
         var other = detached.getContext('2d');
         for (let i = 0; i < 10001; i++) other.fillRect(0, 0, 1, 1);
         document.body.appendChild(detached);",
    );
    let detached = page.engine.dom_mut().read(|cx| {
        let node = cx
            .element_by_id(page.document.get(cx.gc), "detached")
            .expect("appended after");
        cx.node_id(node)
    });
    let (_, layout) = page.engine.dom_and_layout();
    let drawing = layout
        .expect("the layout")
        .as_any_mut()
        .downcast_mut::<Drawing>()
        .expect("the tests'");
    assert_eq!(
        drawing.flushes,
        [
            (canvas, canvas::CanvasFlush::Draw),
            (detached, canvas::CanvasFlush::Draw)
        ]
    );
    assert_eq!(page.data(|data| data.ops.len()), 3);
}

/// The canvases' settings are the options of their first `getContext`.
#[test]
fn the_settings() {
    let mut page = Page::new();
    page.run(
        "var o = document.createElement('canvas');
         o.setAttribute('id', 'o');
         document.body.appendChild(o);
         o.getContext('2d', {alpha: false, colorSpace: 'display-p3', colorType: 'float16',
                             desynchronized: true, willReadFrequently: true});",
    );
    let settings = |page: &mut Page, id: &str| {
        let document = &page.document;
        page.engine.dom_mut().read(|cx| {
            let canvas = cx
                .element_by_id(document.get(cx.gc), id)
                .expect("the canvas");
            cx.canvas_context_settings(cx.canvas_context(canvas).expect("its context"))
        })
    };
    assert_eq!(settings(&mut page, "c"), canvas::CanvasSettings::default());
    assert_eq!(
        settings(&mut page, "o"),
        canvas::CanvasSettings {
            alpha: false,
            desynchronized: true,
            color_space: canvas::PredefinedColorSpace::DisplayP3,
            color_type: canvas::CanvasColorType::Float16,
            will_read_frequently: true,
        }
    );
}

/// `ImageData`'s constructors (an overload told apart by the first
/// argument): of a size, zeroed, in the settings' color space; of an
/// array, that array itself, its height from its length; their errors (an
/// array of another type goes to the size's form, WebIDL's way: its number
/// is NaN, a width of 0). `data` is the same array each time.
#[test]
fn image_data() {
    let mut page = Page::new();
    page.run(
        "var d = new ImageData(2, 3);
         log.push(d instanceof ImageData, String(d), ImageData.length, `${d.width}x${d.height}`,
                  d.data instanceof Uint8ClampedArray, d.data.length, d.data === d.data,
                  d.data.every(b => b === 0), d.colorSpace,
                  new ImageData(1, 1, {colorSpace: 'display-p3'}).colorSpace);
         var a = new Uint8ClampedArray(24);
         var e = new ImageData(a, 2);
         log.push(e.data === a, `${e.width}x${e.height}`, new ImageData(a, 3, 2).height);
         log.push(...[
             () => new ImageData(0, 1), () => new ImageData(1, 0),
             () => new ImageData(1, 1, {colorSpace: 'rec2020'}),
             () => new ImageData(new Uint8ClampedArray(6), 1),
             () => new ImageData(new Uint8ClampedArray(0), 1),
             () => new ImageData(a, 5), () => new ImageData(a, 2, 2), () => new ImageData(a, 0),
             () => new ImageData(a), () => new ImageData(), () => ImageData(1, 1),
             () => new ImageData(new Uint8Array(4), 1), () => new ImageData(1, 1, 1, 1),
             () => new ImageData(65536, 16385),
         ].map(error));",
    );
    assert_eq!(
        page.log(),
        "true / [object ImageData] / 2 / 2x3 / true / 24 / true / true / srgb / display-p3 / \
         true / 2x3 / 2 / \
         IndexSizeError / IndexSizeError / TypeError / InvalidStateError / InvalidStateError / \
         IndexSizeError / IndexSizeError / IndexSizeError / TypeError / TypeError / TypeError / \
         IndexSizeError / TypeError / IndexSizeError"
    );
}

/// `createImageData` (of a size, signs ignored, or of another's size),
/// `putImageData` (its two forms: what isn't 3 or 7 arguments a
/// `TypeError`) and `getImageData` (a negative size going back), round
/// trip: no layout, so the pixels are the context's own; outside the
/// bitmap transparent, an opaque canvas's alpha 255; the color spaces the
/// settings, the image or the context say. Arguments past the longest form
/// ignored; `[EnforceRange]`; a detached array.
#[test]
fn put_and_get_image_data() {
    let mut page = Page::new();
    page.run(
        "var made = ctx.createImageData(-2, 1);
         log.push(`${made.width}x${made.height}`, made.data.every(b => b === 0),
                  ctx.createImageData(made).width, ctx.createImageData(made) !== made);
         made.data.set([255, 0, 0, 255, 0, 255, 0, 128]);
         ctx.putImageData(made, 1, 1);
         log.push(Array.from(ctx.getImageData(1, 1, 2, 1).data).join(','));
         log.push(Array.from(ctx.getImageData(3, 2, -2, -1).data).join(','));
         ctx.putImageData(made, 0, 0, 1, 0, 1, 1);
         log.push(Array.from(ctx.getImageData(0, 0, 2, 1).data).join(','));
         log.push(Array.from(ctx.getImageData(-1, 1, 3, 1).data).join(','));
         log.push(ctx.getImageData(0, 0, 1, 1, {colorSpace: 'display-p3'}).colorSpace,
                  ctx.createImageData(new ImageData(1, 1, {colorSpace: 'display-p3'})).colorSpace);
         var opaque = document.createElement('canvas').getContext('2d', {alpha: false, colorSpace: 'display-p3'});
         opaque.putImageData(made, 0, 0);
         log.push(Array.from(opaque.getImageData(1, 0, 1, 1).data).join(','),
                  opaque.getImageData(0, 0, 1, 1).colorSpace, opaque.createImageData(1, 1).colorSpace);
         var small = document.createElement('canvas');
         small.width = small.height = 2;
         var smallCtx = small.getContext('2d');
         smallCtx.putImageData(made, 1, 0, 0, 0, 1, 1);
         smallCtx.putImageData(made, 0, 1, 0, 0, 1, 1);
         log.push(Array.from(smallCtx.getImageData(-1, 1, 1, 1).data).join(','),
                  Array.from(smallCtx.getImageData(1, -1, 1, 1).data).join(','),
                  ctx.createImageData(1, -3).height);
         ctx.setTransform(1, 0, 0, 1, 0, 0, 9);
         log.push(...[
             () => ctx.putImageData(made, 0, 0, 0), () => ctx.putImageData(made),
             () => ctx.putImageData({}, 0, 0), () => ctx.getImageData(0, 0, 0, 1),
             () => ctx.getImageData(0, 0, 1, 0),
             () => ctx.getImageData(NaN, 0, 1, 1), () => ctx.getImageData(2 ** 31, 0, 1, 1),
             () => ctx.createImageData(0, 1), () => ctx.createImageData(1),
         ].map(error));
         made.data.buffer.transfer();
         log.push(error(() => ctx.putImageData(made, 0, 0)));",
    );
    assert_eq!(
        page.log(),
        "2x1 / true / 2 / true / 255,0,0,255,0,255,0,128 / 255,0,0,255,0,255,0,128 / \
         0,0,0,0,0,255,0,128 / 0,0,0,0,0,0,0,0,255,0,0,255 / display-p3 / display-p3 / \
         0,255,0,255 / display-p3 / display-p3 / 0,0,0,0 / 0,0,0,0 / 3 / \
         TypeError / TypeError / TypeError / IndexSizeError / IndexSizeError / TypeError / TypeError / \
         IndexSizeError / TypeError / InvalidStateError"
    );
}

/// Gradients: of the three kinds, their stops' errors; as a style, the
/// same object back, kept with the state (`save`, `restore`, `reset`), and
/// drawn as a shader.
#[test]
fn gradients() {
    let mut page = Page::new();
    page.run(
        "var g = ctx.createLinearGradient(0, 0, 10, 0);
         log.push(g instanceof CanvasGradient, String(g),
                  ctx.createRadialGradient(0, 0, 1, 0, 0, 2) instanceof CanvasGradient,
                  ctx.createConicGradient(0, 1, 1) instanceof CanvasGradient);
         g.addColorStop(0, 'red'); g.addColorStop(1, 'currentcolor');
         log.push(...[
             () => g.addColorStop(1.5, 'red'), () => g.addColorStop(0.5, 'nope'),
             () => g.addColorStop(NaN, 'red'), () => ctx.createLinearGradient(0, 0, Infinity, 0),
             () => ctx.createRadialGradient(0, 0, -1, 0, 0, 1), () => new CanvasGradient(),
         ].map(error));
         ctx.fillStyle = g;
         log.push(ctx.fillStyle === g);
         ctx.save();
         log.push(ctx.fillStyle === g);
         ctx.fillStyle = 'red';
         ctx.strokeStyle = g;
         log.push(ctx.fillStyle, ctx.strokeStyle === g);
         ctx.restore();
         log.push(ctx.fillStyle === g, ctx.strokeStyle);
         ctx.fillStyle = {toString: () => 'blue'};
         log.push(ctx.fillStyle);
         ctx.fillStyle = g;",
    );
    assert_eq!(
        page.log(),
        "true / [object CanvasGradient] / true / true / IndexSizeError / SyntaxError / TypeError / \
         TypeError / IndexSizeError / TypeError / true / true / #ff0000 / true / true / #000000 / \
         #0000ff"
    );
    page.run("ctx.fillRect(0, 0, 10, 10);");
    let shaded = page.data(|data| {
        data.ops.iter().any(|op| {
            matches!(op, compositor_paint::PaintOp::DrawRect { flags, .. }
                if matches!(flags.shader.as_deref(), Some(compositor_paint::PaintShader::Gradient(_))))
        })
    });
    assert!(shaded, "drawn with the gradient's shader");
    page.run("log = []; ctx.reset(); log.push(ctx.fillStyle);");
    assert_eq!(page.log(), "#000000");
}

/// The shadows' attributes, ignoring what isn't valid; `setTransform`'s
/// dictionary form (none given: the identity; a value's two names
/// differing, a `TypeError`) and its arities; `getContextAttributes`.
#[test]
fn shadows_matrices_and_attributes() {
    let mut page = Page::new();
    page.run(
        "const shadow = () => [ctx.shadowOffsetX, ctx.shadowOffsetY, ctx.shadowBlur,
                               ctx.shadowColor].join(' ');
         log.push(shadow());
         ctx.shadowOffsetX = 2; ctx.shadowOffsetY = -3; ctx.shadowBlur = 4;
         ctx.shadowColor = 'rgba(0, 0, 255, 0.5)';
         ctx.shadowOffsetX = NaN; ctx.shadowBlur = -1; ctx.shadowColor = 'nope';
         log.push(shadow());
         log.push(ctx.setTransform.length,
                  error(() => ctx.setTransform({a: 1, m11: 2})),
                  error(() => ctx.setTransform(1, 2)));
         var attributes = ctx.getContextAttributes();
         log.push(Object.keys(attributes).join(','), JSON.stringify(Object.values(attributes)),
                  attributes !== ctx.getContextAttributes());
         log.push(JSON.stringify(Object.values(document.createElement('canvas').getContext('2d', {
             alpha: false, colorSpace: 'display-p3', colorType: 'float16', desynchronized: true,
             willReadFrequently: true}).getContextAttributes())));",
    );
    assert_eq!(
        page.log(),
        "0 0 0 rgba(0, 0, 0, 0) / 2 -3 4 rgba(0, 0, 255, 0.5) / 0 / TypeError / TypeError / \
         alpha,colorSpace,colorType,desynchronized,willReadFrequently / \
         [true,\"srgb\",\"unorm8\",false,false] / true / \
         [false,\"display-p3\",\"float16\",true,true]"
    );
    page.run("ctx.setTransform({a: 2, d: 3, m41: 1, f: 5});");
    assert_eq!(
        page.data(|data| data.state().transform),
        [2.0, 0.0, 0.0, 3.0, 1.0, 5.0]
    );
    page.run("ctx.setTransform({e: NaN}); ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.scale(2, 2); ctx.setTransform();");
    assert_eq!(
        page.data(|data| data.state().transform),
        canvas::context::IDENTITY
    );
}

/// Every interface the overloads' forms take is one their function tells
/// an object of.
#[test]
fn every_overload_is_told_apart() {
    assert_eq!(dom_js::overload_interfaces_untold(), Vec::<&str>::new());
}

/// The `DrawImage` ops `#c`'s context recorded: their source and
/// destination rects (as `[x, y, w, h]`) and filters.
fn draw_images(page: &mut Page) -> Vec<([f32; 4], [f32; 4], compositor_paint::FilterQuality)> {
    let rect = |r: gfx::RectF| [r.x, r.y, r.width, r.height];
    page.data(|data| {
        data.ops
            .iter()
            .filter_map(|op| match op {
                compositor_paint::PaintOp::DrawImage {
                    src, dst, filter, ..
                } => Some((rect(src.expect("a source rect")), rect(*dst), *filter)),
                _ => None,
            })
            .collect()
    })
}

/// `drawImage`'s three forms from a canvas: the whole of it, scaled, and
/// part of it; nothing for a number that isn't finite or a rect of no
/// size; a `TypeError` for what isn't an `<img>` or a `<canvas>`, an
/// `InvalidStateError` for a canvas of no width or height. The image
/// smoothing's attributes, and the filter they draw with.
#[test]
fn draw_image() {
    let mut page = Page::new();
    page.run(
        "var other = document.createElement('canvas');
         other.width = 4; other.height = 2;
         other.getContext('2d').fillRect(0, 0, 1, 1);
         log.push(ctx.drawImage.length, ctx.imageSmoothingEnabled, ctx.imageSmoothingQuality);
         log.push(...[
             () => ctx.drawImage({}, 0, 0), () => ctx.drawImage(null, 0, 0),
             () => ctx.drawImage(document.body, 0, 0), () => ctx.drawImage(other),
         ].map(error));
         var empty = document.createElement('canvas');
         empty.height = 0;
         log.push(error(() => ctx.drawImage(empty, 0, 0)),
                  error(() => ctx.drawImage(empty, NaN, 0)));
         ctx.drawImage(other, NaN, 0); ctx.drawImage(other, 0, 0, 0, 4);
         ctx.drawImage(other, 0, 0, 4, 2, 0, 0, Infinity, 1);",
    );
    assert_eq!(
        page.log(),
        "3 / true / low / TypeError / TypeError / TypeError / TypeError / InvalidStateError / \
         no error"
    );
    assert_eq!(draw_images(&mut page), [], "none drawn");
    page.run(
        "log = [];
         ctx.drawImage(other, 1, 2);
         ctx.imageSmoothingQuality = 'high'; ctx.imageSmoothingQuality = 'best';
         ctx.drawImage(other, 0, 0, 8, -4);
         ctx.imageSmoothingEnabled = false;
         log.push(ctx.imageSmoothingQuality);
         ctx.drawImage(other, 2, 0, 4, 2, 0, 0, 4, 4);
         ctx.save(); ctx.imageSmoothingEnabled = true; ctx.imageSmoothingQuality = 'medium';
         ctx.restore();
         log.push(ctx.imageSmoothingEnabled, ctx.imageSmoothingQuality);
         ctx.drawImage(document.createElement('canvas'), 0, 0);",
    );
    assert_eq!(page.log(), "high / false / high");
    use compositor_paint::FilterQuality;
    assert_eq!(
        draw_images(&mut page),
        [
            (
                [0.0, 0.0, 4.0, 2.0],
                [1.0, 2.0, 4.0, 2.0],
                FilterQuality::Low
            ),
            (
                [0.0, 0.0, 4.0, 2.0],
                [0.0, -4.0, 8.0, 4.0],
                FilterQuality::High
            ),
            // The source past the image's right edge: the destination cut
            // as much.
            (
                [2.0, 0.0, 2.0, 2.0],
                [0.0, 0.0, 2.0, 4.0],
                FilterQuality::None
            ),
            // A canvas with no context: transparent, at its size.
            (
                [0.0, 0.0, 300.0, 150.0],
                [0.0, 0.0, 300.0, 150.0],
                FilterQuality::None
            ),
        ]
    );
    // `other`, which has no id, drew first.
    assert_eq!(page.draws(), ["", "c"]);
}

/// The URLs the images wait for, the microtasks run first.
fn image_fetches(page: &mut Page) -> Vec<String> {
    page.engine.perform_microtask_checkpoint();
    page.engine
        .dom_mut()
        .take_image_fetches()
        .into_iter()
        .map(|fetch| fetch.url)
        .collect()
}

/// The image of `width` by `height` a host hands back for `url`.
fn image_fetched(page: &mut Page, url: &str, (width, height): (u32, u32), origin_clean: bool) {
    assert_eq!(image_fetches(page), [url]);
    let image = dom::Image::Raster(
        gfx::ImageData::new(vec![0u8; 4]),
        gfx::Size { width, height },
    );
    page.engine
        .dom_mut()
        .mutate(|cx| cx.image_fetched(url, Some(image), origin_clean));
}

/// An `<img>` as the source: nothing drawn (and no pattern) until it is
/// loaded, its natural size over its density, an `InvalidStateError`
/// once broken. One from another origin without CORS leaves a canvas that
/// draws it not origin-clean, and so one that draws that canvas, or fills
/// with a pattern of it (not one that only makes the pattern); a new width
/// makes it clean again.
#[test]
fn images_and_origin_clean() {
    let mut page = Page::new();
    page.run(
        "var img = document.createElement('img');
         img.src = 'a.png';
         var broken = document.createElement('img');
         broken.src = 'b.png';
         ctx.drawImage(img, 0, 0);
         log.push(String(ctx.createPattern(img, 'repeat')));",
    );
    assert_eq!(draw_images(&mut page), []);
    let fetches = image_fetches(&mut page);
    assert_eq!(
        fetches,
        ["https://example.test/a.png", "https://example.test/b.png"]
    );
    page.engine.dom_mut().mutate(|cx| {
        let image = dom::Image::Raster(
            gfx::ImageData::new(vec![0u8; 4]),
            gfx::Size {
                width: 6,
                height: 4,
            },
        );
        cx.image_fetched("https://example.test/a.png", Some(image), false);
        cx.image_fetched("https://example.test/b.png", None, true);
    });
    page.run(
        "log.push(error(() => ctx.drawImage(broken, 0, 0)),
                  error(() => ctx.createPattern(broken, 'repeat')));
         var clean = document.createElement('canvas').getContext('2d');
         var pattern = clean.createPattern(img, 'no-repeat');
         log.push(error(() => clean.getImageData(0, 0, 1, 1)));
         clean.fillStyle = pattern;
         log.push(error(() => clean.getImageData(0, 0, 1, 1)));
         log.push(error(() => ctx.getImageData(0, 0, 1, 1)));
         ctx.drawImage(img, 0, 0);
         log.push(error(() => ctx.getImageData(0, 0, 1, 1)));
         var after = document.createElement('canvas').getContext('2d');
         after.drawImage(c, 0, 0);
         log.push(error(() => after.getImageData(0, 0, 1, 1)));
         ctx.reset();
         log.push(error(() => ctx.getImageData(0, 0, 1, 1)));
         c.width = 300;
         log.push(error(() => ctx.getImageData(0, 0, 1, 1)));",
    );
    assert_eq!(
        page.log(),
        "null / InvalidStateError / InvalidStateError / no error / SecurityError / no error / \
         SecurityError / SecurityError / SecurityError / no error"
    );
    // A same-origin image at density 2 (`srcset`): half its natural size,
    // its source rect in its own pixels.
    page.run("img.srcset = 'c.png 2x'; img.removeAttribute('src');");
    image_fetched(&mut page, "https://example.test/c.png", (6, 4), true);
    page.run(
        "log = [];
         ctx.drawImage(img, 0, 0); ctx.drawImage(img, 1, 1, 1, 1, 0, 0, 2, 2);
         log.push(error(() => ctx.getImageData(0, 0, 1, 1)));",
    );
    assert_eq!(page.log(), "no error");
    let rects: Vec<_> = draw_images(&mut page)
        .into_iter()
        .map(|(src, dst, _)| (src, dst))
        .collect();
    assert_eq!(
        rects,
        [
            ([0.0, 0.0, 6.0, 4.0], [0.0, 0.0, 3.0, 2.0]),
            ([2.0, 2.0, 2.0, 2.0], [0.0, 0.0, 2.0, 2.0]),
        ]
    );
    // Its image data being updated, and its new request pending: nothing
    // to draw (as Chrome's `ImageComplete()`), though it still shows the
    // one it has.
    page.run(
        "log = [];
         img.srcset = 'd.png 2x';
         ctx.drawImage(img, 0, 0);
         log.push(String(ctx.createPattern(img, 'repeat')));",
    );
    assert_eq!(image_fetches(&mut page), ["https://example.test/d.png"]);
    page.run(
        "ctx.drawImage(img, 0, 0);
         log.push(img.complete, String(ctx.createPattern(img, 'repeat')));",
    );
    assert_eq!(page.log(), "null / false / null");
    assert_eq!(draw_images(&mut page).len(), 2, "none drawn since");
    // An image of no width: nothing to draw.
    page.run("img.removeAttribute('srcset'); img.src = 'z.png';");
    image_fetched(&mut page, "https://example.test/z.png", (0, 4), true);
    page.run(
        "log = [];
         ctx.drawImage(img, 0, 0);
         log.push(String(ctx.createPattern(img, 'repeat')));",
    );
    assert_eq!(page.log(), "null");
    assert_eq!(draw_images(&mut page).len(), 2);
}

/// `createPattern`: its repetition checked first (null the empty string),
/// a `CanvasPattern` of a canvas as it is now; `fillStyle` and
/// `strokeStyle` take it, kept with the state; `setTransform`'s
/// dictionary; drawn as a tiled shader, sampled as the image smoothing
/// says.
#[test]
fn patterns() {
    let mut page = Page::new();
    page.run(
        "var other = document.createElement('canvas');
         other.width = 4; other.height = 2;
         var empty = document.createElement('canvas');
         empty.width = 0;
         log.push(error(() => ctx.createPattern(empty, 'diagonal')),
                  error(() => ctx.createPattern(empty, 'repeat')),
                  error(() => ctx.createPattern(other)), error(() => new CanvasPattern()));
         var p = ctx.createPattern(other, null);
         log.push(p instanceof CanvasPattern, String(p), ctx.createPattern(other, 'repeat-y') !== p);
         ctx.fillStyle = p;
         log.push(ctx.fillStyle === p);
         ctx.save();
         log.push(ctx.fillStyle === p);
         ctx.fillStyle = 'red'; ctx.strokeStyle = p;
         log.push(ctx.fillStyle, ctx.strokeStyle === p);
         ctx.restore();
         log.push(ctx.fillStyle === p, ctx.strokeStyle);
         log.push(error(() => p.setTransform({a: 1, m11: 2})), p.setTransform.length);
         p.setTransform({a: 2, f: 3});
         ctx.imageSmoothingEnabled = false;
         ctx.fillRect(0, 0, 10, 10);
         ctx.strokeStyle = ctx.createPattern(other, 'repeat-x');
         ctx.strokeRect(0, 0, 5, 5);
         // A canvas whose size can't have pixels: a pattern of nothing.
         var huge = document.createElement('canvas');
         huge.width = 70000;
         ctx.fillStyle = ctx.createPattern(huge, 'no-repeat');
         ctx.fillRect(0, 0, 1, 1);",
    );
    assert_eq!(
        page.log(),
        "SyntaxError / InvalidStateError / TypeError / TypeError / true / [object CanvasPattern] / \
         true / true / true / #ff0000 / true / true / #000000 / TypeError / 0"
    );
    let shaders = page.data(|data| {
        data.ops
            .iter()
            .filter_map(|op| match op {
                compositor_paint::PaintOp::DrawRect { flags, .. }
                | compositor_paint::PaintOp::DrawPath { flags, .. } => {
                    match flags.shader.as_deref() {
                        Some(compositor_paint::PaintShader::Tiled(tiled)) => Some((
                            tiled.tile,
                            tiled.repeat_x,
                            tiled.repeat_y,
                            tiled.transform,
                            tiled.filter,
                        )),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let tile = gfx::RectF::new(0.0, 0.0, 4.0, 2.0);
    assert_eq!(
        shaders,
        [
            (
                tile,
                true,
                true,
                canvas::context::to_gfx([2.0, 0.0, 0.0, 1.0, 0.0, 3.0]),
                compositor_paint::FilterQuality::None
            ),
            (
                tile,
                true,
                false,
                canvas::context::to_gfx(canvas::context::IDENTITY),
                compositor_paint::FilterQuality::None
            ),
            (
                gfx::RectF::new(0.0, 0.0, 1.0, 1.0),
                false,
                false,
                canvas::context::to_gfx(canvas::context::IDENTITY),
                compositor_paint::FilterQuality::None
            ),
        ]
    );
    page.run("log = []; ctx.reset(); log.push(ctx.fillStyle, ctx.strokeStyle);");
    assert_eq!(page.log(), "#000000 / #000000");
}

/// `toDataURL`: `data:,` with no encoder (no frame), or no pixels; else
/// the file `type` and `quality` say (a quality only if it is a number,
/// from 0 to 1), of the bitmap's pixels as they are, opaque if the context
/// has no alpha; a canvas with no context is transparent; one whose size
/// can't have pixels, `data:,` too.
#[test]
fn to_data_url() {
    let mut page = Page::new();
    page.run("log.push(c.toDataURL());");
    assert_eq!(page.log(), "data:,");
    with_drawing(&mut page);
    page.run(
        "log = [];
         var file = (canvas, ...args) => canvas.toDataURL(...args);
         ctx.fillRect(0, 0, 1, 1);
         log.push(file(c), file(c, 'image/jpeg', 0.5), file(c, 'IMAGE/JPEG', '0.5'),
                  file(c, 'image/jpeg', 1.5), file(c, 'image/webp', 0.5));
         var opaque = document.createElement('canvas');
         opaque.width = 2; opaque.height = 1;
         opaque.getContext('2d', {alpha: false});
         var bare = document.createElement('canvas');
         bare.width = 1; bare.height = 1;
         var empty = document.createElement('canvas');
         empty.width = 0;
         var huge = document.createElement('canvas');
         huge.width = 70000;
         log.push(file(opaque), file(bare), file(empty), file(huge), c.toDataURL.length);",
    );
    let files: Vec<String> = page
        .log()
        .split(" / ")
        .map(|url| match url.split_once(";base64,") {
            Some((head, base64)) => {
                use base64::Engine;
                let file = base64::engine::general_purpose::STANDARD
                    .decode(base64)
                    .expect("base64");
                format!(
                    "{head} {}",
                    String::from_utf8(file).expect("the tests' file")
                )
            }
            None => url.to_owned(),
        })
        .collect();
    assert_eq!(
        files.join(" / "),
        "data:image/png Png 300x150 false 180000 / \
         data:image/jpeg Jpeg { quality: 50 } 300x150 false 180000 / \
         data:image/jpeg Jpeg { quality: 92 } 300x150 false 180000 / \
         data:image/jpeg Jpeg { quality: 92 } 300x150 false 180000 / \
         data:image/png Png 300x150 false 180000 / \
         data:image/png Png 2x1 true 8 / data:image/png Png 1x1 false 4 / data:, / data:, / 0"
    );
    let flushes = page.engine.dom_and_layout().1.expect("the layout");
    let flushes = &flushes
        .as_any_mut()
        .downcast_mut::<Drawing>()
        .expect("the tests'")
        .flushes;
    assert!(
        flushes
            .iter()
            .all(|(_, flush)| *flush == canvas::CanvasFlush::Read),
        "drawn before it is read"
    );
    assert_eq!(flushes.len(), 6, "`c` five times, `opaque` once");
}

/// A bitmap's size and whether it is opaque.
type Bitmap = ((u32, u32), bool);

/// What the host is to encode for each `toBlob` call: its number, the
/// bitmap's size and whether it is opaque (none: no pixels), and the
/// encoding.
fn canvas_encodes(page: &mut Page) -> Vec<(u64, Option<Bitmap>, canvas::export::ImageEncoding)> {
    page.engine
        .dom_mut()
        .take_canvas_encodes()
        .into_iter()
        .map(|encode| {
            (
                encode.id,
                encode.bitmap.map(|bitmap| (bitmap.size, bitmap.opaque)),
                encode.encoding,
            )
        })
        .collect()
}

/// `toBlob`: the bitmap as it is at the call taken, to encode as `type`
/// and `quality` say; once the host hands the file back, the callback is
/// called in an element task with a `Blob` of it (of its file's type), or
/// null for no file (no pixels, or it couldn't be encoded), in the order
/// the files came; what it throws reported. Nothing for a canvas of a
/// document with no window; a callback that isn't a function, or a canvas
/// that isn't origin-clean, throws.
#[test]
fn to_blob() {
    use canvas::export::ImageEncoding;
    let mut page = Page::new();
    page.run(
        "var empty = document.createElement('canvas');
         empty.height = 0;
         var callback = b => log.push(b === null ? 'null' : `${b instanceof Blob} ${b.type} ${b.size}`);
         c.toBlob(callback);
         c.toBlob(callback, 'image/jpeg', 0.25);
         empty.toBlob(callback);
         c.toBlob(() => { throw new Error('thrown'); });
         window.addEventListener('error', e => log.push(e.message.split(': ').pop()));
         var inert = document.implementation.createHTMLDocument().createElement('canvas');
         inert.toBlob(callback);
         log.push(error(() => c.toBlob()), error(() => c.toBlob({})), c.toBlob.length);",
    );
    assert_eq!(page.log(), "TypeError / TypeError / 1");
    assert_eq!(
        canvas_encodes(&mut page),
        [
            (1, Some(((300, 150), false)), ImageEncoding::Png),
            (
                2,
                Some(((300, 150), false)),
                ImageEncoding::Jpeg { quality: 25 }
            ),
            (3, None, ImageEncoding::Png),
            (4, Some(((300, 150), false)), ImageEncoding::Png),
        ]
    );
    assert!(!page.engine.run_element_tasks(), "none till the files come");
    page.engine.dom_mut().mutate(|cx| {
        cx.canvas_encoded(1, Some(b"png".to_vec()));
        cx.canvas_encoded(2, Some(b"jpeg".to_vec()));
        cx.canvas_encoded(3, None);
        cx.canvas_encoded(4, None);
        // A number no call has: nothing.
        cx.canvas_encoded(9, None);
    });
    page.run("log = [];");
    assert!(page.engine.run_element_tasks());
    assert_eq!(
        page.log(),
        "true image/png 3 / true image/jpeg 4 / null / thrown"
    );
    assert!(!page.engine.run_element_tasks(), "each called once");
    page.run("log = []; c.toBlob(b => b.text().then(text => log.push(text)));");
    let [(id, ..)] = canvas_encodes(&mut page)[..] else {
        panic!("one call");
    };
    page.engine
        .dom_mut()
        .mutate(|cx| cx.canvas_encoded(id, Some(b"file".to_vec())));
    page.engine.run_element_tasks();
    assert_eq!(page.log(), "file");
}

/// A canvas that isn't origin-clean can't be exported: `toDataURL` and
/// `toBlob` throw a `SecurityError`, and nothing is encoded.
#[test]
fn a_tainted_canvas_is_not_exported() {
    let mut page = Page::new();
    with_drawing(&mut page);
    page.run("var img = document.createElement('img'); img.src = 'https://other.test/a.png';");
    image_fetched(&mut page, "https://other.test/a.png", (1, 1), false);
    page.run(
        "ctx.drawImage(img, 0, 0);
         log.push(error(() => c.toDataURL()), error(() => c.toBlob(() => {})));",
    );
    assert_eq!(page.log(), "SecurityError / SecurityError");
    assert!(canvas_encodes(&mut page).is_empty());
}

/// `Path2D`: empty, a copy of another (not shared after), or SVG path data
/// (up to an error); its `CanvasPath` methods as the context's (a negative
/// radius an `IndexSizeError`); `addPath` through a transform, itself
/// once; the context fills, strokes and clips to one as it is then, under
/// the current transform.
#[test]
fn paths_2d() {
    let mut page = Page::new();
    page.run(
        "var square = new Path2D('M0 0 H10 V10 H0 Z');
         var copy = new Path2D(square);
         copy.rect(20, 0, 5, 5);
         var broken = new Path2D('M0 0 H10 V10 H0 Z X M20 0 H30');
         var empty = new Path2D();
         log.push(String(square), square instanceof Path2D, error(() => Path2D()),
                  ctx.isPointInPath(square, 5, 5), ctx.isPointInPath(square, 22, 2),
                  ctx.isPointInPath(copy, 22, 2), ctx.isPointInPath(copy, 5, 5),
                  ctx.isPointInStroke(square, 0, 5), ctx.isPointInStroke(square, 5, 5), ctx.isPointInPath(broken, 25, 0.5),
                  ctx.isPointInPath(empty, 0, 0), ctx.isPointInPath(new Path2D(42), 0, 0));
         empty.moveTo(0, 0); empty.lineTo(4, 0); empty.lineTo(4, 4); empty.closePath();
         log.push(ctx.isPointInPath(empty, 3, 1), error(() => empty.arc(0, 0, -1, 0, 1)),
                  error(() => empty.ellipse(0, 0, 1, -1, 0, 0, 1)));
         var moved = new Path2D();
         moved.addPath(square, {e: 100});
         moved.addPath(moved);
         moved.addPath(square, {e: NaN});
         log.push(ctx.isPointInPath(moved, 105, 5), ctx.isPointInPath(moved, 5, 5),
                  error(() => moved.addPath(square, {a: 1, m11: 2})), error(() => moved.addPath({})));
         ctx.translate(1, 0);
         ctx.fill(square, 'evenodd');
         ctx.stroke(square);
         log.push(error(() => ctx.fill(square, 'odd')), ctx.isPointInPath(square, 0.5, 5));
         ctx.clip(square);",
    );
    assert_eq!(
        page.log(),
        "[object Path2D] / true / TypeError / true / false / true / true / true / false / false / \
         false / false / \
         true / IndexSizeError / IndexSizeError / true / false / TypeError / TypeError / \
         TypeError / false"
    );
    let ops = page.data(|data| {
        data.ops
            .iter()
            .filter_map(|op| match op {
                compositor_paint::PaintOp::DrawPath { path, flags, .. } => Some((
                    path.bounds(),
                    matches!(flags.style, compositor_paint::PaintStyle::Stroke { .. }),
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let bounds = gfx::RectF::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(ops, [(Some(bounds), false), (Some(bounds), true),]);
    assert_eq!(page.data(|data| data.state().clips.len()), 1, "clipped");
}

/// `isPointInPath` and `isPointInStroke` of the current default path:
/// the point in the canvas's space, where the path stays as the transform
/// changes, by the fill rule (one that isn't a `CanvasFillRule` a
/// `TypeError`); the stroke by the line styles, under the transform; a
/// point that isn't finite hits nothing.
#[test]
fn hit_tests() {
    let mut page = Page::new();
    page.run(
        "ctx.rect(0, 0, 10, 10);
         ctx.rect(2, 2, 6, 6);
         log.push(ctx.isPointInPath(5, 5), ctx.isPointInPath(5, 5, 'evenodd'),
                  ctx.isPointInPath(1, 5, 'evenodd'), error(() => ctx.isPointInPath(5, 5, 'odd')),
                  ctx.isPointInPath(NaN, 5), ctx.isPointInPath(Infinity, 5));
         ctx.scale(2, 2);
         log.push(ctx.isPointInPath(5, 5), ctx.isPointInPath(15, 15));
         log.push(ctx.isPointInStroke(10, 5), ctx.isPointInStroke(11.5, 5));
         ctx.lineWidth = 2;
         log.push(ctx.isPointInStroke(11.5, 5), ctx.isPointInStroke(NaN, 5));",
    );
    assert_eq!(
        page.log(),
        "true / false / true / TypeError / false / false / true / false / true / false / true / false"
    );
}

/// `roundRect` on the context and on a `Path2D`: one radius for all, a
/// `DOMPointInit`'s `x` and `y`, a list of one to four (another count a
/// `RangeError`, before a negative one is), nothing for one that isn't
/// finite; 0 by default.
#[test]
fn round_rects() {
    let mut page = Page::new();
    page.run(
        "var read = [];
         var point = {get w() { read.push('w'); return 1; }, get x() { read.push('x'); return 3; },
                      get y() { read.push('y'); return 3; }, get z() { read.push('z'); return 0; }};
         var p = new Path2D();
         p.roundRect(0, 0, 10, 10, point);
         log.push(read.join(''), ctx.isPointInPath(p, 0.5, 0.5), ctx.isPointInPath(p, 5, 0.5));
         var q = new Path2D();
         q.roundRect(0, 0, 10, 10, [0, 4]);
         log.push(ctx.isPointInPath(q, 0.5, 0.5), ctx.isPointInPath(q, 9.5, 0.5));
         var r = new Path2D();
         r.roundRect(0, 0, 10, 10);
         log.push(ctx.isPointInPath(r, 0.5, 0.5));
         var nan = new Path2D();
         nan.roundRect(0, 0, 10, 10, [1, NaN]);
         log.push(ctx.isPointInPath(nan, 5, 5));
         log.push(error(() => p.roundRect(0, 0, 1, 1, [])), error(() => p.roundRect(0, 0, 1, 1, [-1, 1, 1, 1, 1])),
                  error(() => p.roundRect(0, 0, 1, 1, -1)), error(() => p.roundRect(0, 0, 1, 1, {x: 1, y: -1})),
                  error(() => p.roundRect(0, 0, 1, 1, null)));
         try { p.roundRect(0, 0, 1, 1, {x: 1, y: -2}); } catch (e) { log.push(e.message); }
         ctx.roundRect(0, 0, 10, 10, 5);
         log.push(ctx.isPointInPath(0.5, 0.5), ctx.isPointInPath(5, 0.5), ctx.roundRect.length);",
    );
    assert_eq!(
        page.log(),
        "wxyz / false / true / true / false / true / false / RangeError / RangeError / RangeError / \
         RangeError / no error / Y-radius value -2 is negative. / false / true / 4"
    );
}

/// The text drawing styles: their defaults, the values they take (an
/// invalid one ignored, as an enumeration's), `inherit` kept; `font` only
/// where a frame shows the canvas's document, computed against 10px
/// sans-serif for a canvas with no style, and read back as Chrome writes
/// it; the fonts parsed through the document's cache (an invalid one too,
/// up to its limit); `reset()` puts them back.
#[test]
fn text_styles() {
    let mut page = Page::new();
    page.run("ctx.font = '20px serif'; log.push(ctx.font);");
    assert_eq!(page.log(), "10px sans-serif", "no frame: not set");
    with_drawing(&mut page);
    page.run(
        "log = [];
         var names = ['font', 'textAlign', 'textBaseline', 'direction', 'letterSpacing', 'wordSpacing',
                      'fontKerning', 'fontStretch', 'fontVariantCaps', 'textRendering'];
         log.push(names.map(name => ctx[name]).join());
         var values = ['bold italic small-caps 2em/3 \"A B\", serif', 'center', 'middle', 'rtl', '0.5em', '2px',
                       'none', 'condensed', 'all-small-caps', 'optimizeSpeed'];
         names.forEach((name, i) => ctx[name] = values[i]);
         log.push(names.map(name => ctx[name]).join());
         ctx.fontVariantCaps = 'normal'; ctx.font = 'small-caps 20px x';
         log.push(ctx.font, ctx.fontVariantCaps);
         ctx.font = values[0]; ctx.fontStretch = values[7]; ctx.fontVariantCaps = values[8];
         names.forEach(name => ctx[name] = 'nope');
         ctx.letterSpacing = '0'; ctx.font = 'inherit'; ctx.font = 'caption';
         log.push(names.map(name => ctx[name]).join());
         ctx.direction = 'inherit'; ctx.font = '12px a';
         log.push(ctx.direction, ctx.font, ctx.fontStretch);
         ctx.reset();
         log.push(names.map(name => ctx[name]).join());",
    );
    assert_eq!(
        page.log(),
        "10px sans-serif,start,alphabetic,inherit,0px,0px,auto,normal,normal,auto / \
         italic bold 20px \"A B\", serif,center,middle,rtl,0.5em,2px,none,condensed,all-small-caps,optimizeSpeed / \
         small-caps 20px x / small-caps / \
         italic bold 20px \"A B\", serif,center,middle,rtl,0.5em,2px,none,condensed,all-small-caps,optimizeSpeed / \
         inherit / 12px a / normal / \
         10px sans-serif,start,alphabetic,inherit,0px,0px,auto,normal,normal,auto"
    );
    let cached = |page: &mut Page| {
        let document = &page.document;
        page.engine
            .dom_mut()
            .read(|cx| cx.cached_canvas_fonts(document.get(cx.gc)))
    };
    // The bold one, the small caps', `nope`, `inherit`, `caption` and
    // `12px a`.
    assert_eq!(cached(&mut page), 6);
    page.run(
        "var other = document.createElement('canvas').getContext('2d');
         other.font = '12px a';
         for (let i = 0; i < 60; i++) other.font = `${i}px a`;",
    );
    assert_eq!(cached(&mut page), dom::MAX_CACHED_FONTS);
}

/// `fillText` and `strokeText`: the glyphs the frame laid out, at `x` as
/// `textAlign` says (`start` and `end` by the direction, `inherit` the
/// canvas's directionality) and `y` as `textBaseline` says, squeezed to a
/// `maxWidth` under a scale; nothing for a number that isn't finite, a
/// `maxWidth` that isn't above 0, or a canvas no frame shows.
#[test]
fn draw_text() {
    let glyphs = |page: &mut Page| {
        page.data(|data| {
            let ops: Vec<_> = data
                .ops
                .iter()
                .filter_map(|op| match op {
                    compositor_paint::PaintOp::DrawGlyphs {
                        positions, flags, ..
                    } => Some((
                        positions.first().copied(),
                        matches!(flags.style, compositor_paint::PaintStyle::Stroke { .. }),
                    )),
                    _ => None,
                })
                .collect();
            data.ops.clear();
            ops
        })
    };
    let mut page = Page::new();
    page.run("ctx.fillText('ab', 0, 0);");
    assert!(glyphs(&mut page).is_empty(), "no frame");
    with_drawing(&mut page);
    page.run(
        "ctx.fillText('ab', 100, 50);
         ctx.textAlign = 'end'; ctx.textBaseline = 'top';
         ctx.strokeText('ab', 100, 50);
         document.documentElement.setAttribute('dir', 'RTL');
         ctx.textAlign = 'start'; ctx.textBaseline = 'bottom';
         ctx.fillText('ab', 100, 50);
         // `auto`: the first strong character, not in a `textarea`.
         document.documentElement.setAttribute('dir', 'auto');
         document.body.insertBefore(document.createTextNode('\\u05d0'), c);
         document.body.insertBefore(document.createElement('textarea'), document.body.firstChild)
             .appendChild(document.createTextNode('a'));
         ctx.fillText('ab', 100, 50);
         document.body.insertBefore(document.createTextNode('a'), document.body.firstChild);
         ctx.fillText('ab', 100, 50);
         ctx.direction = 'ltr';
         ctx.fillText('ab', 100, 50);
         ctx.fillText('ab', NaN, 50); ctx.fillText('ab', 0, 0, 0); ctx.fillText('ab', 0, 0, -1);
         ctx.fillText('', 0, 0);",
    );
    let p = |x, y| Some(gfx::PointF::new(x, y));
    assert_eq!(
        glyphs(&mut page),
        [
            (p(100.0, 50.0), false),
            (p(80.0, 57.0), true),
            (p(80.0, 47.0), false),
            (p(80.0, 47.0), false),
            (p(100.0, 47.0), false),
            (p(100.0, 47.0), false),
            // The tests' layout gives the empty text a run of no glyphs.
            (None, false),
        ]
    );
    page.run("ctx.textBaseline = 'alphabetic'; ctx.fillText('abcd', 10, 0, 20);");
    // Squeezed by half, the left end is at 20 in the scaled space.
    assert_eq!(glyphs(&mut page), [(p(20.0, 0.0), false)]);
}

/// `measureText`: a new `TextMetrics` of the text as `fillText` would lay
/// it out, its numbers from the alignment point and the `textBaseline`'s
/// line; all 0 where no frame shows the canvas's document.
#[test]
fn measure_text() {
    let mut page = Page::new();
    let metrics = "var m = ctx.measureText('abc');
         log.push(m instanceof TextMetrics, m !== ctx.measureText('abc'), m.width, m.actualBoundingBoxLeft,
                  m.actualBoundingBoxRight, m.fontBoundingBoxAscent, m.fontBoundingBoxDescent,
                  m.actualBoundingBoxAscent, m.actualBoundingBoxDescent, m.hangingBaseline,
                  m.alphabeticBaseline, m.ideographicBaseline, error(() => new TextMetrics()));";
    page.run(metrics);
    assert_eq!(
        page.log(),
        "true / true / 0 / 0 / 0 / 0 / 0 / 0 / 0 / 0 / 0 / 0 / TypeError"
    );
    with_drawing(&mut page);
    page.run(&format!(
        "log = []; ctx.textAlign = 'center'; ctx.textBaseline = 'top'; {metrics}"
    ));
    assert_eq!(
        page.log(),
        "true / true / 30 / 14 / 14 / 1 / 9 / 0 / 8 / -0.5999999046325684 / -7 / -9 / TypeError"
    );
}
