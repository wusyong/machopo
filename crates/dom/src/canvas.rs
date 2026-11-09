//! `<canvas>` (Blink's `HTMLCanvasElement`) and its 2D context (Blink's
//! `CanvasRenderingContext2D`) on the DOM: the element keeps its
//! context in its `ElementData` (Blink's `HTMLCanvasElement::context_`),
//! made by the first `getContext("2d")` and the same one after; the
//! context points back at its element, and keeps its settings and its data
//! (the `canvas` crate's [`CanvasData`]: the state
//! stack, the ops recorded and not drawn yet, the bitmap). HTML's
//! algorithms are `dom_api`'s (with `canvas::context`'s), drawing
//! the ops the renderer's. A state's style that is a gradient or a pattern
//! is in the context's slots, two a level (Blink's
//! `CanvasRenderingContext2DState` holds its `CanvasStyle`'s gradient or
//! pattern), kept in step with the state stack (`save_canvas_state`,
//! `restore_canvas_state`, `reset_canvas_context`).
//!
//! `ImageData`, `CanvasGradient` and `CanvasPattern` are the context's
//! script objects: an `ImageData`'s pixels are the engine's array, which
//! it holds (a `JsEdge`: Blink's `ImageData` holds its
//! `DOMUint8ClampedArray`); a gradient's geometry and stops, and a
//! pattern's image and tiling, are plain data, taken out and put back.
//!
//! A `toBlob` call waits for its file (`request_canvas_blob`): the host
//! takes what to encode (`Dom::take_canvas_encodes`), encodes it off the
//! main thread, and hands the file back (`canvas_encoded`), which queues
//! the element task that calls the callback with a `Blob` of it.

use std::cell::Cell;

use canvas::export::{ExportBitmap, ImageEncoding};
use canvas::{
    CanvasColorStop, CanvasData, CanvasGradientKind, CanvasPatternData, CanvasSettings,
    CanvasStyle, CanvasStyleTarget, DEFAULT_CANVAS_HEIGHT, DEFAULT_CANVAS_WIDTH,
    PredefinedColorSpace, TextMetricsValues, parse_non_negative_integer,
};
use elise::{External, Gc, GcObject, HeapVector, JsEdge, Member, Persistent};

use crate::{Blob, BlobData, Cx, Dom, Node, NodeId, NodeRef, names, sites};

/// A canvas's 2D context.
#[derive(GcObject)]
pub struct CanvasRenderingContext2d {
    /// Its canvas element (HTML's `canvas` attribute), which keeps it.
    canvas: Member<Node>,
    settings: External<Box<CanvasSettings>>,
    /// Taken and put back (a move, no borrow).
    data: External<Cell<Option<Box<CanvasData>>>>,
    /// Each level's styles' gradients and patterns, fill and stroke
    /// (`style_slot`), for every level the stack has had: those above the
    /// current one empty.
    styles: Member<HeapVector<CanvasStyleSlot>>,
    wrapper: JsEdge,
}

/// One of a level's two styles' objects: its gradient or its pattern, if
/// it is one (the other empty).
#[derive(GcObject)]
pub struct CanvasStyleSlot {
    gradient: Member<CanvasGradient>,
    pattern: Member<CanvasPattern>,
}

/// A style's object, a gradient or a pattern.
#[derive(Clone, Copy)]
pub enum CanvasStyleObject<'gc> {
    Gradient(Gc<'gc, CanvasGradient>),
    Pattern(Gc<'gc, CanvasPattern>),
}

/// A `CanvasGradient` (Blink's): its geometry and stops, added to by
/// `addColorStop`; of no realm (no window keeps it).
#[derive(GcObject)]
pub struct CanvasGradient {
    /// Taken and put back.
    data: External<Cell<Option<Box<CanvasGradientData>>>>,
    wrapper: JsEdge,
}

/// A `CanvasPattern` (Blink's): its image as it was when made, how it
/// tiles, and its transform (`setTransform`); of no realm.
#[derive(GcObject)]
pub struct CanvasPattern {
    /// Taken and put back.
    data: External<Cell<Option<Box<CanvasPatternData>>>>,
    wrapper: JsEdge,
}

/// A `Path2D` (Blink's `Path2D`): its path, which its `CanvasPath`
/// methods build and a context fills, strokes, clips to or hit tests as it
/// is then; taken out and put back; of no realm.
#[derive(GcObject)]
pub struct Path2d {
    path: External<Cell<Option<Box<gfx::Path>>>>,
    wrapper: JsEdge,
}

/// A `TextMetrics` (Blink's): `measureText`'s numbers, as they were when it
/// was made; of no realm.
#[derive(GcObject)]
pub struct TextMetrics {
    values: External<TextMetricsValues>,
    wrapper: JsEdge,
}

/// The fonts a document's canvases were given, parsed (Blink's
/// `CanvasFontCache`): a context that is set one again, or another
/// canvas's, doesn't parse it again; none for a string that isn't a font.
/// Up to `MAX_CACHED_FONTS`, the first parsed let go first.
#[derive(Default)]
pub struct CanvasFontCache {
    fonts: std::collections::HashMap<String, Option<css::SpecifiedCanvasFont>>,
    order: std::collections::VecDeque<String>,
}

/// How many fonts a document's cache keeps (Blink's
/// `CanvasFontCache::MaxFonts` for a page in the foreground).
pub const MAX_CACHED_FONTS: usize = 50;

/// A gradient's geometry, and its stops in the order they were added.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasGradientData {
    pub kind: CanvasGradientKind,
    pub stops: Vec<CanvasColorStop>,
}

/// An `ImageData` (Blink's): its size, color space, and its pixels, the
/// engine's `Uint8ClampedArray` (set as it is made, by the front end).
#[derive(GcObject)]
pub struct ImageData {
    width: u32,
    height: u32,
    /// `PredefinedColorSpace` as a byte (`color_space_code`).
    color_space: u8,
    data: JsEdge,
    wrapper: JsEdge,
}

/// A `toBlob` call waiting for its file (HTML's "serialize as a file"):
/// its number, its canvas, its callback (the engine's function), the file's
/// type (as an `ImageEncoding`'s, `JPEG` or not), and the blob once the
/// file came (none for no file).
#[derive(GcObject)]
pub struct CanvasBlobCallback {
    id: u64,
    canvas: Member<Node>,
    jpeg: bool,
    blob: Member<Blob>,
    callback: JsEdge,
}

/// What a `toBlob` call is to encode (`Dom::take_canvas_encodes`): the
/// call's number, the bitmap (none for one with no pixels: no file), and
/// the encoding.
#[derive(Debug, Clone)]
pub struct CanvasEncode {
    pub id: u64,
    pub bitmap: Option<ExportBitmap>,
    pub encoding: ImageEncoding,
}

fn color_space_code(space: PredefinedColorSpace) -> u8 {
    match space {
        PredefinedColorSpace::Srgb => 0,
        PredefinedColorSpace::DisplayP3 => 1,
    }
}

/// The slot of `level`'s `target` style.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "level < states.len() <= isize::MAX, so level * 2 + 1 <= usize::MAX"
)]
fn style_slot(level: usize, target: CanvasStyleTarget) -> usize {
    level * 2
        + match target {
            CanvasStyleTarget::Fill => 0,
            CanvasStyleTarget::Stroke => 1,
        }
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `element` is an HTML `<canvas>`.
    pub fn is_canvas(&self, element: NodeRef<'gc>) -> bool {
        self.is_html_named(element, names::local::CANVAS)
    }

    /// A `<canvas>`'s bitmap size, in CSS px (Blink's
    /// `HTMLCanvasElement::Size`): its `width` and `height` attributes,
    /// each by HTML's rules for non-negative integers, 300 and 150 where
    /// missing or not one.
    pub fn canvas_size(&self, element: NodeRef<'gc>) -> (u32, u32) {
        let dimension = |name, default| {
            self.attribute_value_by_id(element, name)
                .and_then(|value| parse_non_negative_integer(&value.borrow().to_str()))
                .unwrap_or(default)
        };
        (
            dimension(names::local::WIDTH, DEFAULT_CANVAS_WIDTH),
            dimension(names::local::HEIGHT, DEFAULT_CANVAS_HEIGHT),
        )
    }

    /// `canvas`'s 2D context, if `getContext("2d")` made it one.
    pub fn canvas_context(
        &self,
        canvas: NodeRef<'gc>,
    ) -> Option<Gc<'gc, CanvasRenderingContext2d>> {
        self.existing_element_data(canvas)?
            .canvas_context
            .get(self.gc)
    }

    /// `canvas`'s 2D context made, with `settings`, for a bitmap of its
    /// size.
    pub fn create_canvas_context(
        &self,
        canvas: NodeRef<'gc>,
        settings: CanvasSettings,
    ) -> Gc<'gc, CanvasRenderingContext2d> {
        let data = CanvasData::new(self.canvas_size(canvas));
        let styles = HeapVector::new(self.gc, 2);
        for _ in 0..2 {
            styles.push(self.gc, self.new_style_slot());
        }
        let context = self.gc.alloc_in(
            &sites::OTHER,
            CanvasRenderingContext2dInit {
                canvas: Some(canvas),
                settings: External::new(Box::new(settings)),
                data: External::new(Cell::new(Some(Box::new(data)))),
                styles: Some(styles),
                wrapper: (),
            },
        );
        self.element_data(canvas)
            .canvas_context
            .set(self.gc, Some(context));
        context
    }

    pub fn canvas_context_canvas(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
    ) -> NodeRef<'gc> {
        context.canvas.get(self.gc).expect("a context's canvas")
    }

    pub fn canvas_context_settings(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
    ) -> CanvasSettings {
        **context.into_ref().settings
    }

    pub(crate) fn canvas_context_wrapper_edge(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
    ) -> &'gc JsEdge {
        &context.into_ref().wrapper
    }

    /// `f` on `context`'s data (taken out and put back: `f` can't reach
    /// it again).
    ///
    /// # Panics
    ///
    /// If `f` asks for the same context's data.
    pub fn with_canvas_data<R>(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
        f: impl FnOnce(&mut CanvasData) -> R,
    ) -> R {
        let cell = context.into_ref().data.get();
        let mut data = cell.take().expect("a context's data, not taken already");
        let result = f(&mut data);
        cell.set(Some(data));
        result
    }

    /// `canvas`'s context drew, or its bitmap was reset: the renderer
    /// draws it before the next frame (`take_canvas_draws`), and asks for
    /// that frame.
    pub fn record_canvas_draw(&self, canvas: NodeRef<'gc>) {
        let id = self.node_id(canvas);
        let mut draws = self.changes.canvas_draws.borrow_mut();
        if !draws.contains(&id) {
            draws.push(id);
        }
        let version = &self.changes.canvas_draws_version;
        version.set(version.get().wrapping_add(1));
    }

    /// How many times canvases drew: the rendering is asked for when it
    /// changes.
    pub fn canvas_draws_version(&self) -> u64 {
        self.changes.canvas_draws_version.get()
    }

    /// The canvases that drew since last taken, in the order they first
    /// did.
    pub fn take_canvas_draws(&self) -> Vec<NodeId> {
        std::mem::take(&mut *self.changes.canvas_draws.borrow_mut())
    }

    /// `canvases` keep waiting to be drawn (not shown in a frame yet),
    /// before any that drew since.
    pub fn keep_canvas_draws(&self, canvases: Vec<NodeId>) {
        let mut draws = self.changes.canvas_draws.borrow_mut();
        let since = std::mem::replace(&mut *draws, canvases);
        for id in since {
            if !draws.contains(&id) {
                draws.push(id);
            }
        }
    }

    fn new_style_slot(&self) -> Gc<'gc, CanvasStyleSlot> {
        self.gc.alloc_in(
            &sites::OTHER,
            CanvasStyleSlotInit {
                gradient: None,
                pattern: None,
            },
        )
    }

    fn style_slots(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
    ) -> Gc<'gc, HeapVector<CanvasStyleSlot>> {
        context.styles.get(self.gc).expect("a context's slots")
    }

    fn slot(
        &self,
        slots: Gc<'gc, HeapVector<CanvasStyleSlot>>,
        level: usize,
        target: CanvasStyleTarget,
    ) -> Gc<'gc, CanvasStyleSlot> {
        slots
            .get(self.gc, style_slot(level, target))
            .expect("a slot for each level the stack has had")
    }

    /// `save()`: the state pushed (`canvas::context::save`), its styles'
    /// gradients and patterns with it.
    pub fn save_canvas_state(&self, context: Gc<'gc, CanvasRenderingContext2d>) {
        let (below, level) = self.with_canvas_data(context, |data| {
            let below = data.level();
            canvas::context::save(data);
            (below, data.level())
        });
        let slots = self.style_slots(context);
        while slots.len() <= style_slot(level, CanvasStyleTarget::Stroke) {
            slots.push(self.gc, self.new_style_slot());
        }
        for target in [CanvasStyleTarget::Fill, CanvasStyleTarget::Stroke] {
            let (from, to) = (
                self.slot(slots, below, target),
                self.slot(slots, level, target),
            );
            to.gradient.set(self.gc, from.gradient.get(self.gc));
            to.pattern.set(self.gc, from.pattern.get(self.gc));
        }
    }

    /// `restore()`: the state popped (`canvas::context::restore`), its
    /// styles' gradients and patterns let go of; `false` (and nothing) with none pushed.
    pub fn restore_canvas_state(&self, context: Gc<'gc, CanvasRenderingContext2d>) -> bool {
        let popped = self.with_canvas_data(context, |data| {
            let popped = data.level();
            canvas::context::restore(data).then_some(popped)
        });
        let Some(popped) = popped else {
            return false;
        };
        self.clear_canvas_styles(context, popped..=popped);
        true
    }

    /// HTML's "reset the rendering context to its default state", for a
    /// bitmap of `size` (`CanvasData::reset`), every level's gradients and
    /// patterns let go of.
    pub fn reset_canvas_context(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
        size: (u32, u32),
    ) {
        let levels = self.with_canvas_data(context, |data| {
            let levels = data.states.len();
            data.reset(size);
            levels
        });
        self.clear_canvas_styles(context, 0..levels);
    }

    fn clear_canvas_styles(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
        levels: impl IntoIterator<Item = usize>,
    ) {
        let slots = self.style_slots(context);
        for level in levels {
            for target in [CanvasStyleTarget::Fill, CanvasStyleTarget::Stroke] {
                if let Some(slot) = slots.get(self.gc, style_slot(level, target)) {
                    slot.gradient.set(self.gc, None);
                    slot.pattern.set(self.gc, None);
                }
            }
        }
    }

    /// The current state's `target` style's gradient or pattern, if it is
    /// one.
    pub fn canvas_style_object(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
        target: CanvasStyleTarget,
    ) -> Option<CanvasStyleObject<'gc>> {
        let level = self.with_canvas_data(context, |data| data.level());
        let slot = self.slot(self.style_slots(context), level, target);
        match slot.gradient.get(self.gc) {
            Some(gradient) => Some(CanvasStyleObject::Gradient(gradient)),
            None => slot.pattern.get(self.gc).map(CanvasStyleObject::Pattern),
        }
    }

    /// The current state's `target` style made `style`, its gradient or
    /// pattern (if it is one: `CanvasStyle::Gradient`, `Pattern`) kept in
    /// its slot.
    pub fn set_canvas_style(
        &self,
        context: Gc<'gc, CanvasRenderingContext2d>,
        target: CanvasStyleTarget,
        style: CanvasStyle,
        object: Option<CanvasStyleObject<'gc>>,
    ) {
        let level = self.with_canvas_data(context, |data| {
            let state = data.state_mut();
            match target {
                CanvasStyleTarget::Fill => state.fill_style = style,
                CanvasStyleTarget::Stroke => state.stroke_style = style,
            }
            data.level()
        });
        let slot = self.slot(self.style_slots(context), level, target);
        let (gradient, pattern) = match object {
            Some(CanvasStyleObject::Gradient(gradient)) => (Some(gradient), None),
            Some(CanvasStyleObject::Pattern(pattern)) => (None, Some(pattern)),
            None => (None, None),
        };
        slot.gradient.set(self.gc, gradient);
        slot.pattern.set(self.gc, pattern);
    }

    /// A new `CanvasGradient` of `kind`, with no stops.
    pub fn create_canvas_gradient(&self, kind: CanvasGradientKind) -> Gc<'gc, CanvasGradient> {
        let data = CanvasGradientData {
            kind,
            stops: Vec::new(),
        };
        self.gc.alloc_in(
            &sites::OTHER,
            CanvasGradientInit {
                data: External::new(Cell::new(Some(Box::new(data)))),
                wrapper: (),
            },
        )
    }

    /// `f` on `gradient`'s data (taken out and put back).
    ///
    /// # Panics
    ///
    /// If `f` asks for the same gradient's data.
    pub fn with_canvas_gradient<R>(
        &self,
        gradient: Gc<'gc, CanvasGradient>,
        f: impl FnOnce(&mut CanvasGradientData) -> R,
    ) -> R {
        let cell = gradient.into_ref().data.get();
        let mut data = cell.take().expect("a gradient's data, not taken already");
        let result = f(&mut data);
        cell.set(Some(data));
        result
    }

    pub(crate) fn canvas_gradient_wrapper_edge(
        &self,
        gradient: Gc<'gc, CanvasGradient>,
    ) -> &'gc JsEdge {
        &gradient.into_ref().wrapper
    }

    /// A new `CanvasPattern` of `data`.
    pub fn create_canvas_pattern(&self, data: CanvasPatternData) -> Gc<'gc, CanvasPattern> {
        self.gc.alloc_in(
            &sites::OTHER,
            CanvasPatternInit {
                data: External::new(Cell::new(Some(Box::new(data)))),
                wrapper: (),
            },
        )
    }

    /// `f` on `pattern`'s data (taken out and put back).
    ///
    /// # Panics
    ///
    /// If `f` asks for the same pattern's data.
    pub fn with_canvas_pattern<R>(
        &self,
        pattern: Gc<'gc, CanvasPattern>,
        f: impl FnOnce(&mut CanvasPatternData) -> R,
    ) -> R {
        let cell = pattern.into_ref().data.get();
        let mut data = cell.take().expect("a pattern's data, not taken already");
        let result = f(&mut data);
        cell.set(Some(data));
        result
    }

    pub(crate) fn canvas_pattern_wrapper_edge(
        &self,
        pattern: Gc<'gc, CanvasPattern>,
    ) -> &'gc JsEdge {
        &pattern.into_ref().wrapper
    }

    /// A new `TextMetrics` of `values`.
    pub fn create_text_metrics(&self, values: TextMetricsValues) -> Gc<'gc, TextMetrics> {
        self.gc.alloc_in(
            &sites::OTHER,
            TextMetricsInit {
                values: External::new(values),
                wrapper: (),
            },
        )
    }

    pub fn text_metrics(&self, metrics: Gc<'gc, TextMetrics>) -> TextMetricsValues {
        *metrics.into_ref().values.get()
    }

    pub(crate) fn text_metrics_wrapper_edge(&self, metrics: Gc<'gc, TextMetrics>) -> &'gc JsEdge {
        &metrics.into_ref().wrapper
    }

    /// `text` as a canvas's `font` (`css::parse_canvas_font`), through
    /// `document`'s cache (`CanvasFontCache`); parsed each time for a node
    /// that isn't a document.
    pub fn parse_canvas_font(
        &self,
        document: NodeRef<'gc>,
        text: &str,
    ) -> Option<css::SpecifiedCanvasFont> {
        let Some(data) = self.document_data(document) else {
            return css::parse_canvas_font(text);
        };
        let cell = data.into_ref().canvas_fonts.get();
        let mut cache = cell.take().unwrap_or_default();
        let parsed = match cache.fonts.get(text) {
            Some(parsed) => parsed.clone(),
            None => {
                let parsed = css::parse_canvas_font(text);
                if cache.order.len() >= MAX_CACHED_FONTS
                    && let Some(oldest) = cache.order.pop_front()
                {
                    cache.fonts.remove(&oldest);
                }
                cache.fonts.insert(text.to_owned(), parsed.clone());
                cache.order.push_back(text.to_owned());
                parsed
            }
        };
        cell.set(Some(cache));
        parsed
    }

    /// How many fonts `document`'s cache has. For tests.
    pub fn cached_canvas_fonts(&self, document: NodeRef<'gc>) -> usize {
        let Some(data) = self.document_data(document) else {
            return 0;
        };
        let cell = data.into_ref().canvas_fonts.get();
        let cache = cell.take();
        let count = cache.as_ref().map_or(0, |cache| cache.fonts.len());
        cell.set(cache);
        count
    }

    /// A new `Path2D` of `path`.
    pub fn create_path_2d(&self, path: gfx::Path) -> Gc<'gc, Path2d> {
        self.gc.alloc_in(
            &sites::OTHER,
            Path2dInit {
                path: External::new(Cell::new(Some(Box::new(path)))),
                wrapper: (),
            },
        )
    }

    /// `f` on `path`'s path (taken out and put back).
    ///
    /// # Panics
    ///
    /// If `f` asks for the same path's.
    pub fn with_path_2d<R>(&self, path: Gc<'gc, Path2d>, f: impl FnOnce(&mut gfx::Path) -> R) -> R {
        let cell = path.into_ref().path.get();
        let mut taken = cell.take().expect("a path's, not taken already");
        let result = f(&mut taken);
        cell.set(Some(taken));
        result
    }

    pub(crate) fn path_2d_wrapper_edge(&self, path: Gc<'gc, Path2d>) -> &'gc JsEdge {
        &path.into_ref().wrapper
    }

    /// A new `ImageData` of `width` × `height` pixels in `space`, its array
    /// not set yet: the front end sets it (`image_data_array_edge`).
    pub fn create_image_data(
        &self,
        width: u32,
        height: u32,
        space: PredefinedColorSpace,
    ) -> Gc<'gc, ImageData> {
        self.gc.alloc_in(
            &sites::OTHER,
            ImageDataInit {
                width,
                height,
                color_space: color_space_code(space),
                data: (),
                wrapper: (),
            },
        )
    }

    pub fn image_data_size(&self, image_data: Gc<'gc, ImageData>) -> (u32, u32) {
        let image_data = image_data.into_ref();
        (image_data.width, image_data.height)
    }

    pub fn image_data_color_space(&self, image_data: Gc<'gc, ImageData>) -> PredefinedColorSpace {
        if image_data.into_ref().color_space == color_space_code(PredefinedColorSpace::DisplayP3) {
            PredefinedColorSpace::DisplayP3
        } else {
            PredefinedColorSpace::Srgb
        }
    }

    /// Where `image_data`'s pixels are: the engine's array, which it holds.
    pub fn image_data_array_edge(&self, image_data: Gc<'gc, ImageData>) -> &'gc JsEdge {
        &image_data.into_ref().data
    }

    pub(crate) fn image_data_wrapper_edge(&self, image_data: Gc<'gc, ImageData>) -> &'gc JsEdge {
        &image_data.into_ref().wrapper
    }

    /// A `<canvas>`'s `width` or `height` changed (HTML: its context, if
    /// any, reset to its default state, its bitmap the new size and clean
    /// again (origin-clean), even if the value is the same): drawn again,
    /// and laid out at its new natural size.
    pub(crate) fn canvas_size_changed(&self, canvas: NodeRef<'gc>) {
        if let Some(context) = self.canvas_context(canvas) {
            let size = self.canvas_size(canvas);
            self.reset_canvas_context(context, size);
            self.with_canvas_data(context, |data| data.origin_clean = true);
            self.record_canvas_draw(canvas);
        }
        self.set_needs_reattach(canvas);
    }
}

impl Dom {
    /// What the `toBlob` calls since the last ask are to encode, in order:
    /// the host encodes each (off the main thread, in order) and hands the
    /// file back (`Cx::canvas_encoded`).
    pub fn take_canvas_encodes(&self) -> Vec<CanvasEncode> {
        self.changes.canvas_encodes.take()
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// A `toBlob` call of `canvas` (whose document has a window), to encode
    /// `bitmap` as `encoding` (`Dom::take_canvas_encodes`): the edge its
    /// callback goes in, which the front end sets.
    pub fn request_canvas_blob(
        &self,
        canvas: NodeRef<'gc>,
        bitmap: Option<ExportBitmap>,
        encoding: ImageEncoding,
    ) -> &'gc JsEdge {
        let made = &self.changes.canvas_blobs_made;
        let id = made.get().wrapping_add(1);
        made.set(id);
        let callback = self.gc.alloc_in(
            &sites::OTHER,
            CanvasBlobCallbackInit {
                id,
                canvas: Some(canvas),
                jpeg: matches!(encoding, ImageEncoding::Jpeg { .. }),
                blob: None,
                callback: (),
            },
        );
        let waiting = &self.changes.canvas_blobs;
        let list = match waiting.take() {
            Some(list) => list,
            None => Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
        };
        list.get(self.gc).push(self.gc, callback);
        waiting.set(Some(list));
        let mut encodes = self.changes.canvas_encodes.take();
        encodes.push(CanvasEncode {
            id,
            bitmap,
            encoding,
        });
        self.changes.canvas_encodes.set(encodes);
        &callback.into_ref().callback
    }

    /// The waiting `toBlob` call `id` names (taken out if `take`).
    fn find_canvas_blob(&self, id: u64, take: bool) -> Option<Gc<'gc, CanvasBlobCallback>> {
        let waiting = &self.changes.canvas_blobs;
        let list = waiting.take()?;
        let callbacks = list.get(self.gc);
        let found = (0..callbacks.len()).find_map(|index| {
            let callback = callbacks.get(self.gc, index)?;
            (callback.id == id).then_some((index, callback))
        });
        if take && let Some((index, _)) = found {
            callbacks.remove(self.gc, index);
        }
        waiting.set(Some(list));
        found.map(|(_, callback)| callback)
    }

    /// The host encoded the bitmap of the `toBlob` call `id` names
    /// (`Dom::take_canvas_encodes`): `file`, or none if it couldn't. A
    /// `Blob` of it made, of its type, and the element task that calls the
    /// callback with it queued; the call dropped if its canvas's document
    /// has no window now.
    pub fn canvas_encoded(&self, id: u64, file: Option<Vec<u8>>) {
        let Some(callback) = self.find_canvas_blob(id, false) else {
            return;
        };
        let callback = callback.into_ref();
        if let Some(bytes) = file {
            let encoding = if callback.jpeg {
                "image/jpeg"
            } else {
                "image/png"
            };
            let blob = self.create_blob(BlobData::new(bytes, encoding.to_owned()));
            callback.blob.set(self.gc, Some(blob));
        }
        let canvas = callback.canvas.get(self.gc).expect("a call's canvas");
        if !self.queue_canvas_blob_task(canvas, id) {
            self.find_canvas_blob(id, true);
        }
    }

    /// The `toBlob` call `id` names, its file come, taken out
    /// (`ElementTaskKind::CanvasBlob`'s task): what its callback is called
    /// with (`canvas_blob_parts`).
    pub fn take_canvas_blob(&self, id: u64) -> Option<Gc<'gc, CanvasBlobCallback>> {
        self.find_canvas_blob(id, true)
    }

    /// A `toBlob` call's callback, and the blob it is called with (none:
    /// null).
    pub fn canvas_blob_parts(
        &self,
        callback: Gc<'gc, CanvasBlobCallback>,
    ) -> (&'gc JsEdge, Option<Gc<'gc, Blob>>) {
        let callback = callback.into_ref();
        (&callback.callback, callback.blob.get(self.gc))
    }
}
