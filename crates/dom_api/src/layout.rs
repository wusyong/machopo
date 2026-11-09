//! What a member reaches its document's frame's layout by (Blink's
//! `Document::UpdateStyleAndLayout` and the `LocalFrameView` it finds):
//! the host's (the renderer's), which the engine holds and lends each
//! operation (`ApiCx::with_layout`). CSSOM View's members bring style and
//! layout up to date through it, then read what layout gave (an element's
//! box, its fragments' rects, the viewport, what is at a point).

use std::any::Any;
use std::cell::Cell;
use std::sync::Arc;

use computed_style::ComputedStyle;
use dom::{Cx, NodeId, NodeRef, NodeRoot};
use gfx::{ElementBox, Path, PointF, RectF, SizeF, SvgGeometry, SvgTextChar, Viewport};

use crate::FrameId;

/// What a hit test found at a point (`FrameLayout::hit_test`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hit {
    /// The topmost element there; none outside every box.
    pub element: Option<NodeId>,
    /// The text node, if a run of the element's text was hit (`element`
    /// is the element whose style it has, its parent in the flat tree).
    pub text: Option<NodeId>,
}

/// A frame tree's layout as its documents' members ask it: the host's.
pub trait FrameLayout: Any {
    /// Whether one of its frames shows `document`: the frame the queries
    /// that follow are of (a node's box, the viewport, what is at a
    /// point), until the next call.
    fn shows(&mut self, document: NodeId) -> bool;

    /// Style and layout of the document brought up to date, in the
    /// member's operation (Blink's `Document::UpdateStyleAndLayout`).
    fn update_style_and_layout(&mut self, cx: &Cx<'_, '_>);

    /// Style of the document brought up to date, nothing laid out (Blink's
    /// `Document::UpdateStyleAndLayoutTreeForElement`): what
    /// `getComputedStyle` reads of what layout doesn't decide.
    fn update_style(&mut self, cx: &Cx<'_, '_>) {
        self.update_style_and_layout(cx);
    }

    /// `node`'s box as last laid out, if it has one.
    fn element_box(&self, node: NodeId) -> Option<ElementBox>;

    /// `node`'s boxes' border boxes as last laid out, CSS px of the
    /// viewport (scrolled), in order; empty without one.
    fn client_rects(&self, node: NodeId) -> Vec<RectF>;

    /// The boxes of text `text`'s data from `start` to `end` (UTF-16 code
    /// units), CSS px of the viewport (scrolled, unless `element`, the one
    /// it is in, is fixed to it), as last laid out (Blink's
    /// `LayoutText::AbsoluteQuadsForRange`); empty without a box. None by
    /// default.
    fn text_rects(
        &self,
        text: NodeId,
        element: Option<NodeId>,
        start: u32,
        end: u32,
    ) -> Vec<RectF> {
        let _ = (text, element, start, end);
        Vec::new()
    }

    /// The first place a caret can be in editing host `host`, as last laid
    /// out (`layout::first_caret_position`: a text and an offset, or a
    /// parent and a child's index); its start by default.
    fn first_caret_position<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        host: NodeRef<'gc>,
    ) -> (NodeRef<'gc>, u32) {
        let _ = cx;
        (host, 0)
    }

    /// The frame's viewport as last laid out.
    fn viewport(&self) -> Viewport;

    /// The frame's size, CSS px, for `innerWidth` and `innerHeight`: what
    /// decides it laid out, not the document's own layout where that
    /// doesn't (Blink's `LocalDOMWindow::GetViewportSize`). By default, the
    /// viewport as it is, nothing laid out.
    fn viewport_size(&mut self, cx: &Cx<'_, '_>) -> SizeF {
        let _ = cx;
        self.viewport().size
    }

    /// Child frame `frame` made for iframe `owner` in the frame showing
    /// `parent`, its document `document` (whose id is `document_node`):
    /// shown at once, so that script reading its layout right after
    /// inserting the iframe finds it (Blink makes the `LocalFrameView` as
    /// the iframe is inserted). Nothing by default.
    fn child_frame_created(
        &mut self,
        frame: FrameId,
        parent: NodeId,
        owner: NodeId,
        document: NodeRoot,
        document_node: NodeId,
    ) {
        let _ = (frame, parent, owner, document, document_node);
    }

    /// Whether the frame's viewport is known: a child frame's once it was
    /// laid out (at its iframe's content box). Known by default.
    fn has_viewport(&self) -> bool {
        true
    }

    /// Where the browser window the frame is in is on the screen and how
    /// big, CSS px (Blink's `ChromeClient::RootWindowRect`): none in no
    /// browser window.
    fn window_screen_rect(&self) -> Option<RectF>;

    /// `node`'s computed style as last styled, if it has one.
    fn computed_style<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<Arc<ComputedStyle>>;

    /// `element`'s `pseudo`'s style as last styled, if it has one (one no
    /// rule gives content, none).
    fn pseudo_style<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        pseudo: computed_style::PseudoId,
    ) -> Option<Arc<ComputedStyle>> {
        let _ = (cx, element, pseudo);
        None
    }

    /// The element at `point` (CSS px of the viewport) as laid out, the
    /// topmost: pre-painted first if it isn't; none outside the document.
    fn element_at(&mut self, point: PointF) -> Option<NodeId>;

    /// `point` (CSS px of the top-level frame's viewport, the frame tree's
    /// layout lent for the top-level document) in the viewport of the frame
    /// showing `document` (Blink's `ConvertFromRootFrame`: through its
    /// iframes' transforms and scrolling); none if no frame of the tree
    /// shows it (none by default, for a layout of one frame).
    fn point_in_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        let _ = (document, point);
        None
    }

    /// What the pointer at `point` is over, as `element_at` finds it, and
    /// whether that is a run of the element's own text (the I-beam's,
    /// Blink's `HitTestResult::InnerNode` being a text node). By default,
    /// `element_at`'s element and no text.
    fn hit_test(&mut self, point: PointF) -> Hit {
        Hit {
            element: self.element_at(point),
            text: None,
        }
    }

    /// `point_in_frame` the other way: `point` in the viewport of the frame
    /// showing `document` in the top-level frame's (Blink's
    /// `ConvertToRootFrame`).
    fn point_from_frame(&mut self, document: NodeId, point: PointF) -> Option<PointF> {
        let _ = (document, point);
        None
    }

    /// Every element at `point`, the topmost first (`elementsFromPoint`'s
    /// hit test, WebDriver's in-view check): pre-painted first.
    fn elements_at(&mut self, point: PointF) -> Vec<NodeId>;

    /// An SVG element's geometry as last laid out (Blink's
    /// `SVGGraphicsElement` reading its layout object): the default (no
    /// box, identity matrices) if nothing around it is laid out.
    fn svg_geometry<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> SvgGeometry {
        let _ = (cx, element);
        SvgGeometry::default()
    }

    /// An SVG shape's path in its user space, for its length
    /// (`SVGGeometryElement`, Blink's `AsPath`): as laid out; one not
    /// rendered from its style; `None` without a style, where only a
    /// `<path>`'s data gives one.
    fn svg_shape_path<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Option<Path> {
        let _ = (cx, element);
        None
    }

    /// Whether `point`, in an SVG shape's user space, is in its fill area,
    /// or its stroke area (`stroke`): `false` for one with no layout node.
    fn svg_point_in_shape<'gc>(
        &self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        point: PointF,
        stroke: bool,
    ) -> bool {
        let _ = (cx, element, point, stroke);
        false
    }

    /// The characters of an SVG `<text>`, or of a `<tspan>` or `<a>` in
    /// one, as laid out, in logical order; none for one not rendered.
    fn svg_text_chars<'gc>(&self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) -> Vec<SvgTextChar> {
        let _ = (cx, element);
        Vec::new()
    }

    /// Scrolls `scroller` (a scroll container of the frame showing
    /// `document`, or `document` for its viewport) to `offset`, CSS px,
    /// clamped to what it can scroll as last laid out (Blink's
    /// `ScrollableArea::SetScrollOffset`). Nothing by default.
    fn scroll_to(&mut self, document: NodeId, scroller: NodeId, offset: PointF) {
        let _ = (document, scroller, offset);
    }

    /// Scrolls `element` into view as `params` say, in each scrolling box
    /// around it through the frames around its own (Blink's
    /// `ScrollRectToVisible`), the frames laid out already. Nothing by
    /// default.
    fn scroll_into_view<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        element: NodeRef<'gc>,
        params: gfx::ScrollIntoViewParams,
    ) {
        let _ = (cx, element, params);
    }

    /// Scrolls the caret of `document` into view as `scroll_into_view`
    /// does an element (Blink's `FrameSelection::RevealSelection`): the
    /// caret of its focused text control, or of the collapsed selection in
    /// its focused editing host, whether or not the page has the focus.
    /// Whether it has one: nothing if not. Nothing by default.
    fn scroll_caret_into_view<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        document: NodeRef<'gc>,
        params: gfx::ScrollIntoViewParams,
    ) -> bool {
        let _ = (cx, document, params);
        false
    }

    /// A scrolling `key` (the DOM's `key`: an arrow, PageUp, PageDown,
    /// Home, End, Space; Shift held: `shift`) scrolls the first box around
    /// `chain` that can move that way (the focused element and its
    /// ancestors, each frame's document among them, out through the
    /// frames), else the top-level viewport, by the key's step for it
    /// (Blink's `ScrollManager::LogicalScroll`); the frame tree's layout,
    /// lent for the top-level document. Whether anything moved: nothing
    /// for another key, and nothing by default.
    fn keyboard_scroll(&mut self, chain: &[NodeId], key: &str, shift: bool) -> bool {
        let _ = (chain, key, shift);
        false
    }

    /// Scrolls `element` into view in its scroll container `container`
    /// only (a list box's option; Blink's `ScrollToOption`), once the
    /// document is next laid out. Nothing by default.
    fn scroll_into_view_in(&mut self, container: NodeId, element: NodeId) {
        let _ = (container, element);
    }

    /// Where `node`'s border box is painted, CSS px from its document's
    /// origin as last pre-painted (Blink's `FragmentData::PaintOffset`):
    /// what a device-pixel size snaps by. None without a box, or before
    /// the first pre-paint.
    fn paint_offset(&self, node: NodeId) -> Option<PointF> {
        let _ = node;
        None
    }

    /// The platform's system colors the document is styled with: what a
    /// canvas's colors parse a system color as. The defaults by default.
    fn system_colors(&self) -> gfx::SystemColors {
        gfx::SystemColors::DEFAULT
    }

    /// Draws what `canvas`'s context recorded onto its bitmap now, or sends
    /// it to the thread that has the bitmap, as `flush` says (too much
    /// recorded; its pixels read next). Nothing by default.
    fn flush_canvas(&mut self, cx: &Cx<'_, '_>, canvas: NodeId, flush: canvas::CanvasFlush) {
        let _ = (cx, canvas, flush);
    }

    /// A canvas's bitmap (`canvas::export::ExportBitmap`) as an image file
    /// of `encoding`, for `toDataURL`, on this thread, its pixels waited
    /// for if another thread fills them in (Blink's `ImageEncoder`): `None`
    /// if they never come or can't be encoded, and by default (a host with
    /// no encoders).
    fn encode_image(
        &mut self,
        bitmap: &canvas::export::ExportBitmap,
        encoding: canvas::export::ImageEncoding,
    ) -> Option<Vec<u8>> {
        let _ = (bitmap, encoding);
        None
    }

    /// `text` laid out on one line for a canvas's 2D context, in `style`
    /// (Blink's `Font` for the context's state, which both draws and
    /// measures it): the glyphs, the width and the first font's metrics,
    /// and where the glyphs put ink if `ink` (only `measureText` needs it,
    /// as in Blink). By default `None`: a host with no fonts, whose
    /// contexts draw no text and measure none.
    fn shape_canvas_text(
        &mut self,
        text: &str,
        style: &computed_style::CanvasTextStyle,
        ink: bool,
    ) -> Option<gfx::ShapedText> {
        let _ = (text, style, ink);
        None
    }

    /// The host's own type, for the host (`ScriptEngine::layout`).
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Where the engine keeps the host's layout, lent to an operation by a
/// move (taken out and put back: no borrow flag), shared or whole alike.
pub type LayoutCell = Cell<Option<Box<dyn FrameLayout>>>;

/// What an operation borrows the host's layout from (`ApiCx`): the
/// engine's cell, or a layout the host lends for one call of its own.
/// Either lends it by a move, so a borrow inside another finds none.
pub(crate) trait LendLayout {
    /// `f` with the layout, if it isn't lent already.
    fn lend(&self, f: &mut dyn FnMut(&mut dyn FrameLayout));
}

impl LendLayout for LayoutCell {
    fn lend(&self, f: &mut dyn FnMut(&mut dyn FrameLayout)) {
        if let Some(mut layout) = self.take() {
            f(&mut *layout);
            self.set(Some(layout));
        }
    }
}

/// A layout the host lends an operation of its own (DevTools' `CSS`
/// agent reading what `getComputedStyle` reads).
pub(crate) struct LentLayout<'l>(Cell<Option<&'l mut dyn FrameLayout>>);

impl<'l> LentLayout<'l> {
    pub(crate) fn new(layout: &'l mut dyn FrameLayout) -> Self {
        Self(Cell::new(Some(layout)))
    }
}

impl LendLayout for LentLayout<'_> {
    fn lend(&self, f: &mut dyn FnMut(&mut dyn FrameLayout)) {
        if let Some(layout) = self.0.take() {
            f(&mut *layout);
            self.0.set(Some(layout));
        }
    }
}
