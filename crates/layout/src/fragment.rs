//! The immutable fragment tree, layout's output (`blink::PhysicalBoxFragment`).
//! Plain data off the GC heap: fragments point back to DOM nodes by id.

use std::fmt::Write;
use std::sync::Arc;

use computed_style::{BorderStyle, ComputedStyle, PseudoId, Sides};
use gfx::NodeId;
use gfx::{Color, FontData, ImageData, PointF, RectF, SizeF};

use crate::svg::SvgFragment;

/// One box's layout result.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxFragment {
    /// The element that generated the box; `None` for the `LayoutView`
    /// (the initial containing block) and anonymous blocks.
    pub node: Option<NodeId>,
    /// The pseudo-element of `node` that generated it, if one did: hit
    /// testing finds the element, but it isn't the element's own box.
    pub pseudo: Option<PseudoId>,
    pub role: BoxRole,
    pub style: Arc<ComputedStyle>,
    /// It floats (CSS 2 § 9.5): a float in a block container, not a flex or
    /// grid item, whose `float` does nothing. Painted whole after the
    /// backgrounds of the blocks around it, before their lines (CSS 2
    /// Appendix E step 5).
    pub floating: bool,
    /// The border box.
    pub size: SizeF,
    pub padding: Sides<f32>,
    pub border: Sides<f32>,
    pub margin: Sides<f32>,
    /// Its block-level children; or, if it is an inline formatting
    /// context's root, its atomic inlines, which its lines place. Then the
    /// out-of-flow boxes it is the containing block of, laid out against
    /// it, wherever they are below it.
    pub children: Vec<FragmentLink>,
    /// The out-of-flow boxes whose parent it is, where they are in tree
    /// order among its children: what they paint with, in the stacking
    /// context and under the effects of the elements around them there,
    /// though they are laid out against their containing blocks, whose
    /// children they are (Blink paints by its `PaintLayer` tree, which
    /// follows the layout tree, not the containing blocks).
    pub out_of_flow: Vec<OutOfFlowPlace>,
    /// The line boxes, if this box is an inline formatting context's root.
    pub lines: Vec<LineFragment>,
    /// A replaced element's image, drawn into its content box; `None` for
    /// other boxes, and while the image is loading or broken.
    pub image: Option<ImageData>,
    /// A canvas's box (Blink's `LayoutHTMLCanvas`): paint draws its 2D
    /// context's bitmap in its content box, which the canvas, not layout,
    /// has.
    pub canvas: bool,
    /// An outermost `<svg>`'s content, laid out in its content box.
    pub svg: Option<Arc<SvgFragment>>,
    /// An SVG image drawn into its content box: the renderer's number for
    /// it, which paints it at that size.
    pub svg_image: Option<u64>,
    /// Where a replaced element's content (an image, a canvas's bitmap,
    /// an SVG image) is drawn, from the border box's origin: its content
    /// box, or where `object-fit` and `object-position` put it (which may
    /// reach past the content box); `None` for other boxes, and for an
    /// outermost `<svg>`, which lays its content out by its `viewBox`.
    pub object_rect: Option<RectF>,
    /// A table's column groups and columns, which have no fragments of
    /// their own: their backgrounds are painted in the cells they span.
    pub columns: Vec<TableColumn>,
    /// Hidden by layout: a collapsed table row (`visibility: collapse`),
    /// and a cell all of whose columns are collapsed. Neither it nor what is
    /// in it is painted or hit, though they are laid out (Blink's
    /// `IsHiddenForPaint`).
    pub hidden: bool,
    /// `position: fixed` with no box around it its containing block (a
    /// `transform` or layout or paint containment makes one): laid out
    /// against the viewport, it doesn't scroll with the document (Blink's
    /// `LayoutBox::IsFixedToView`). One a box around contains scrolls with
    /// it, under it in this tree.
    pub fixed_to_viewport: bool,
    /// A box below it is sticky with an inset (Blink's
    /// `PropagatedData::sticky_descendants`): what moves as a scrollport
    /// scrolls is found under the boxes that say so.
    pub sticky_below: bool,
    /// What its `clip-path` clips it and what is in it to, from its border
    /// box's top left corner; `None` for nothing.
    pub clip_path: Option<crate::ClipPaths>,
    /// A table with collapsed borders, or one of its cells: its border is
    /// part of them, which the table paints (Blink's
    /// `HasCollapsedBorders`); `empty-cells` does nothing.
    pub border_collapsed: bool,
    /// A cell with collapsed borders that it, its row or its row group
    /// paints apart from its table (positioned, a stacking context): the
    /// backgrounds painted in it stay inside its border, not to cover the
    /// borders its table painted before (Blink's
    /// `TableCellBackgroundClipper`).
    pub background_inside_border: bool,
    /// A table's collapsed borders, which it paints after its cells'
    /// backgrounds.
    pub collapsed_borders: Option<Arc<CollapsedBorders>>,
    /// For a box with a native look, the states it shows.
    pub control_state: Option<ControlState>,
}

/// The states a box's native look shows, as its element has them (Blink's
/// `ControlStates`, which `LayoutTheme::ControlStatesForNode` reads at
/// paint): a part of a control's UA shadow tree (its clear button, its
/// slider thumb) is checked, disabled or read-only as its control, hovered
/// and pressed as itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlState {
    pub checked: bool,
    pub indeterminate: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
}

/// A table's collapsed borders as its layout resolved and placed them
/// (Blink's `TableBorders` with `CollapsedTableBordersGeometry`).
#[derive(Debug, Clone, PartialEq)]
pub struct CollapsedBorders {
    pub rows: usize,
    pub columns: usize,
    /// For each slot of the grid, and one more row and column, the edge on
    /// its left, then the one on its top.
    pub edges: Vec<CollapsedBorder>,
    /// From the table's border box: each row's top, then the last's bottom.
    pub row_offsets: Vec<f32>,
    /// Each column's left, then the last's right.
    pub column_offsets: Vec<f32>,
}

/// One edge of a table's collapsed borders: the border that won it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollapsedBorder {
    pub width: f32,
    /// As collapsed borders draw it: `inset` is `ridge` there, `outset`
    /// `groove`.
    pub style: BorderStyle,
    pub color: Color,
    /// When the box it is from was visited: at a joint between two
    /// otherwise equal edges, the lower order's is drawn.
    pub box_order: u32,
}

impl CollapsedBorder {
    /// Whether it draws anything (Blink's `CanPaint`).
    pub fn can_paint(&self) -> bool {
        self.style.is_drawn() && self.width > 0.0
    }
}

/// A table's column or column group where its table laid it out (Blink's
/// `TableFragmentData::ColumnGeometry`): its columns' extent, across the
/// rows.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    pub node: Option<NodeId>,
    pub style: Arc<ComputedStyle>,
    /// A column group's: painted under its columns'.
    pub group: bool,
    /// From the table's border box.
    pub rect: RectF,
}

/// A line box (`blink::PhysicalLineBoxFragment`). Coordinates are relative
/// to the border box of the block the line is in.
#[derive(Debug, Clone, PartialEq)]
pub struct LineFragment {
    /// From the alignment offset, without white space hanging at the end.
    pub rect: RectF,
    pub runs: Vec<TextFragment>,
    /// The part on this line of each inline element with content or an
    /// edge on it, in tree order.
    pub boxes: Vec<InlineBoxFragment>,
    /// The atomic inlines on this line, in order.
    pub atomic_inlines: Vec<AtomicInline>,
    /// Hidden by a line clamp: a line after the clamped one, laid out but
    /// neither painted nor hit, nor what is on it (Blink's
    /// `IsHiddenForPaint`).
    pub hidden: bool,
    /// How it ends, and the box a selection over its end draws (Blink
    /// paints a selected line end as a space's box): `None` for a line
    /// without text.
    pub end: Option<LineEnd>,
}

/// How a line ends (`LineFragment::end`), and the box a selection over
/// its end draws.
#[derive(Debug, Clone, PartialEq)]
pub struct LineEnd {
    /// The box's width: a space's advance in the font of the text before
    /// it, or the `<br>`'s.
    pub width: f32,
    /// That font's baseline, ascent and descent: the box is as tall as
    /// that or the line, whichever reaches further, as selected text is.
    pub baseline: f32,
    pub ascent: f32,
    pub descent: f32,
    /// The style its colors are from, and the element it is painted as:
    /// the text's, or the `<br>`'s (whose element is the block's).
    pub style: Arc<ComputedStyle>,
    pub node: Option<NodeId>,
    pub(crate) at: LineEndAt,
}

/// What ends a line, as layout leaves it: what depends on the clusters of
/// the text before it is worked out when asked, as few lines' ends are
/// ever selected or have the caret.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LineEndAt {
    /// A `<br>`, its box at `x`.
    Break {
        br: NodeId,
        x: f32,
    },
    /// A newline kept in the text, its box where it is or where an atomic
    /// inline last on the line ends.
    Newline {
        text: TextEnd,
        atomic_end: Option<f32>,
    },
    /// A soft wrap after the text, or the end of the content.
    Wrap(TextEnd),
    End(TextEnd),
}

/// The text a line ends with: its node, the line's last run with text in
/// it (its glyphs, its left edge, and where its content ends, white space
/// hanging past the line aside), and where the run's part of the data
/// starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TextEnd {
    pub(crate) node: NodeId,
    pub(crate) glyphs: TextGlyphs,
    pub(crate) x: f32,
    pub(crate) end: f32,
    pub(crate) start: u32,
}

impl TextEnd {
    /// The last cluster (`TextFragment::clusters`) that starts before the
    /// content's end.
    fn last_kept(&self) -> Option<&TextCluster> {
        self.glyphs
            .clusters()
            .iter()
            .rev()
            .find(|cluster| self.x + cluster.x < self.end)
    }
}

impl LineEnd {
    /// Where the box is: after the line's text, before the edges of the
    /// elements that end with it (Chrome 152).
    pub fn x(&self) -> f32 {
        match &self.at {
            LineEndAt::Break { x, .. } => *x,
            LineEndAt::Newline { text, atomic_end } => atomic_end.unwrap_or_else(|| {
                text.glyphs
                    .clusters()
                    .last()
                    .map_or(text.end, |newline| text.x + newline.x)
            }),
            LineEndAt::Wrap(text) | LineEndAt::End(text) => text.end,
        }
    }

    /// What ends the line.
    pub fn kind(&self) -> LineEndKind {
        match &self.at {
            LineEndAt::Break { br, .. } => LineEndKind::Break(*br),
            LineEndAt::Newline { text, .. } => LineEndKind::Newline {
                text: text.node,
                offset: text
                    .glyphs
                    .clusters()
                    .last()
                    .map_or(text.start, |newline| newline.start),
            },
            // White space hanging past the line has no room: its clusters
            // are where the run ends.
            LineEndAt::Wrap(text) => LineEndKind::Wrap {
                text: text.node,
                offset: text.last_kept().map_or(text.start, |cluster| cluster.end),
            },
            LineEndAt::End(text) => LineEndKind::End {
                text: text.node,
                offset: text.last_kept().map_or(text.start, |cluster| cluster.end),
            },
        }
    }

    /// The `<br>` that ends the line, if one does: without working out the
    /// rest of what ends it.
    pub fn br(&self) -> Option<NodeId> {
        match self.at {
            LineEndAt::Break { br, .. } => Some(br),
            _ => None,
        }
    }

    /// The text node whose text ends the line, if one does: without working
    /// out where in it.
    pub fn text(&self) -> Option<NodeId> {
        match &self.at {
            LineEndAt::Break { .. } => None,
            LineEndAt::Newline { text, .. } | LineEndAt::Wrap(text) | LineEndAt::End(text) => {
                Some(text.node)
            }
        }
    }
}

/// What ends a line, which says when a selection is over its end. The
/// offsets are in the data of text node `text`, the line's last (UTF-16).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineEndKind {
    /// A soft wrap after the text up to `offset` (white space hanging past
    /// it isn't): a selection from before `offset` to past it is over it.
    Wrap { text: NodeId, offset: u32 },
    /// The end of the content, after the text up to `offset`: a selection
    /// from before `offset` on to what is laid out after the text node is
    /// over it, one ending in the node isn't.
    End { text: NodeId, offset: u32 },
    /// A newline kept in the text, at `offset`: a selection with it in is
    /// over it.
    Newline { text: NodeId, offset: u32 },
    /// A `<br>`: a selection with it in is over it.
    Break(NodeId),
}

/// An atomic inline (an `inline-block`, an `<img>`) on a line: one of the
/// block's children, its fragment whole, painted where it comes in the
/// line (a box item of Blink's `FragmentItems`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtomicInline {
    /// Its index in the block's `children`.
    pub child: usize,
    /// Paints right before `runs[paint_before]`, as inline boxes do,
    /// after the first `after_boxes` of `boxes` and before the others: in
    /// tree order, after the elements it is in and those before it, and
    /// before those that start after it.
    pub paint_before: usize,
    pub after_boxes: usize,
    /// Hidden by `text-overflow`: laid out, not painted or hit (Blink's
    /// `LayoutObject::IsTruncated`).
    pub hidden: bool,
}

/// An inline element's box on one line (a `blink::PhysicalBoxFragment` of
/// an inline box). An element that wraps has one per line, sliced
/// (`box-decoration-break: slice`): only the first has its start border
/// and padding, only the last its end ones.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineBoxFragment {
    pub node: NodeId,
    /// The pseudo-element of `node` the box is, if it is one.
    pub pseudo: Option<PseudoId>,
    pub style: Arc<ComputedStyle>,
    /// The border box's part on this line: horizontally from its start
    /// edge (or its first content here) to its end edge (or its last);
    /// vertically the font's content area plus padding and border, which
    /// may reach into the lines around it.
    pub rect: RectF,
    /// The border to draw on this line: the start or end side is 0 unless
    /// the element starts or ends here.
    pub border: Sides<f32>,
    /// The padding on this line, the same way: what its content box is.
    pub padding: Sides<f32>,
    /// Whether the element starts on this line (its start edge, border
    /// and corners are here), and whether it ends on it.
    pub first: bool,
    pub last: bool,
    /// Paints right before `runs[paint_before]` (after them all when it is
    /// `runs.len()`), so the line paints in tree order as CSS 2 Appendix E
    /// says: an element's background and border, then its text and its
    /// children's boxes in order.
    pub paint_before: usize,
}

impl TextFragment {
    /// The run moved by `by`: its glyphs and clusters are placed from its
    /// start.
    pub(crate) fn translate(&mut self, by: PointF) {
        self.x += by.x;
        self.baseline += by.y;
    }

    /// Calls `f` with the id and origin (on the baseline) of each of its
    /// first `count` glyphs, placed as asked: paint places every glyph of
    /// every run in each paint, so this is a plain loop (Blink's
    /// `ShapeResult::ForEachGlyph`).
    pub fn for_each_glyph(&self, count: usize, mut f: impl FnMut(u32, PointF)) {
        let (x, baseline) = (self.x, self.baseline);
        match &self.glyphs {
            TextGlyphs::Shaped(span) => {
                let mut offset = 0.0;
                for g in span.span.glyphs(&span.shaped).take(count) {
                    f(g.id, PointF::new(x + (g.x + offset), baseline + g.y));
                    offset += g.advance;
                }
            }
            TextGlyphs::Owned(glyphs) => {
                for &(id, at) in glyphs.iter().take(count) {
                    f(id, PointF::new(x + at.x, baseline + at.y));
                }
            }
        }
    }

    /// Its glyph ids and origins (on the baseline), placed when asked.
    pub fn glyphs(&self) -> std::vec::IntoIter<(u32, PointF)> {
        let mut glyphs = Vec::new();
        self.for_each_glyph(usize::MAX, |id, at| glyphs.push((id, at)));
        glyphs.into_iter()
    }

    /// Its clusters in order, placed from its start (`x`): the boxes of a
    /// part of the run (Blink's `ShapeResultView::LocalRect`). Worked out
    /// the first time they are asked for; none for a run of no text node.
    pub fn clusters(&self) -> &[TextCluster] {
        self.glyphs.clusters()
    }

    /// Where its first cluster (in `clusters`' order) starts: from its
    /// clusters if they were worked out, else that one alone, so that a
    /// look at each run of an editing host (a caret's first place, as the
    /// host takes the focus) doesn't work out every cluster of its text.
    pub fn first_cluster_start(&self) -> Option<u32> {
        match &self.glyphs {
            TextGlyphs::Shaped(span) => match span.clusters.get() {
                Some(clusters) => clusters.first().map(|cluster| cluster.start),
                None => span
                    .source
                    .as_ref()
                    .and_then(|source| source.first_cluster_start(&span.span, &span.shaped)),
            },
            TextGlyphs::Owned(_) => None,
        }
    }

    /// Whether its clusters were worked out: layout leaves them to what
    /// asks for them, but for `text-overflow`.
    pub fn clusters_made(&self) -> bool {
        match &self.glyphs {
            TextGlyphs::Shaped(span) => span.clusters.get().is_some(),
            TextGlyphs::Owned(_) => false,
        }
    }
}

/// Where a text run's glyphs and clusters come from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TextGlyphs {
    /// A span of its paragraph's shaped text, which its lines share
    /// (Blink's `ShapeResultView` of a `ShapeResult`).
    Shaped(Arc<ShapedSpan>),
    /// Its own glyph ids and origins, from its start on its baseline: a
    /// soft hyphen's hyphen, an ellipsis. It has no clusters.
    Owned(Arc<[(u32, PointF)]>),
}

impl TextGlyphs {
    fn clusters(&self) -> &[TextCluster] {
        match self {
            TextGlyphs::Shaped(span) => span.clusters(),
            TextGlyphs::Owned(_) => &[],
        }
    }
}

/// A glyph run's span of its paragraph's shaped text, and the text node
/// it is of (its data's UTF-16 offsets), with its clusters once made.
pub(crate) struct ShapedSpan {
    pub(crate) shaped: Arc<parley::ShapedText>,
    pub(crate) span: parley::GlyphSpan,
    pub(crate) source: Option<Arc<crate::text::TextSource>>,
    pub(crate) clusters: std::sync::OnceLock<Box<[TextCluster]>>,
}

impl ShapedSpan {
    fn clusters(&self) -> &[TextCluster] {
        self.clusters.get_or_init(|| match &self.source {
            Some(source) => source.clusters(&self.span, &self.shaped),
            None => Box::default(),
        })
    }
}

/// The same glyphs and clusters of the same text, by value: a tree built
/// again shapes its own. The same span of the same shaped text is that
/// without walking them.
impl PartialEq for ShapedSpan {
    fn eq(&self, other: &Self) -> bool {
        let source = match (&self.source, &other.source) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            (a, b) => a.is_none() && b.is_none(),
        };
        let (a, b) = (&self.span, &other.span);
        source
            && ((Arc::ptr_eq(&self.shaped, &other.shaped) && a == b)
                || (a.glyphs(&self.shaped).eq(b.glyphs(&other.shaped))
                    && a.clusters(&self.shaped).eq(b.clusters(&other.shaped))))
    }
}

/// Not its shaped text, which is the paragraph's.
impl std::fmt::Debug for ShapedSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapedSpan")
            .field("span", &self.span)
            .field("clusters", &self.clusters.get())
            .finish_non_exhaustive()
    }
}

/// One glyph run: text of one font and style on one line
/// (`blink::PhysicalTextFragment`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextFragment {
    /// The element the text is in (hit testing finds links through it).
    pub node: Option<NodeId>,
    /// The pseudo-element of `node` that generated the text, if one did
    /// (it has no text node, so no `source`).
    pub pseudo: Option<PseudoId>,
    /// The text node the run is from, and the part of its data the run
    /// stands for, in UTF-16 code units: a node's runs, in order, cover all
    /// of it (white space that collapsed away goes with the run before
    /// it). `None` for a `<br>`'s.
    pub source: Option<(NodeId, std::ops::Range<u32>)>,
    /// Its glyphs and clusters (`Self::glyphs`, `Self::clusters`).
    pub(crate) glyphs: TextGlyphs,
    pub font: FontData,
    pub font_size: f32,
    /// Synthesized bold, and oblique angle in degrees (0 for none).
    pub embolden: bool,
    pub skew: f32,
    /// A variable font's axes as the run was shaped with them; empty for
    /// another font.
    pub variations: Arc<[gfx::FontVariation]>,
    /// Where the run starts, and its advance without white space that
    /// hangs at the line's end (that is removed, so gets no decorations).
    pub x: f32,
    pub advance: f32,
    /// What `text-overflow` hides: the glyphs (`Self::glyphs`) from this
    /// one on aren't
    /// painted or hit, and the decorations stop at `visible_advance`
    /// (`advance` without it). The geometry stays: layout queries see the
    /// run as if it weren't truncated (Blink's hidden-for-paint item next
    /// to its truncated copy).
    pub hidden_from: Option<usize>,
    pub visible_advance: f32,
    pub baseline: f32,
    /// Font metrics for decorations, in px: ascent and descent from the
    /// baseline, and each decoration's offset (upwards from the baseline to
    /// its top edge) and thickness.
    pub ascent: f32,
    pub descent: f32,
    pub underline_offset: f32,
    pub underline_size: f32,
    pub strikethrough_offset: f32,
    pub strikethrough_size: f32,
    /// The style of the text's element: color and decorations.
    pub style: Arc<ComputedStyle>,
}

impl TextFragment {
    /// Where the part `start..end` of its text node's data is in the run,
    /// from left to right (clipped to the run's part): from where the
    /// cluster `start` is in starts to where the next cluster after `end`
    /// starts (a point inside a cluster takes all of it), else the run's
    /// end; a point in white space that collapsed away is where the next
    /// character is. White space that hangs past the line has no room. An
    /// empty range is where its point is. Left to right only, as machopo lays
    /// out.
    pub fn x_range(&self, start: u32, end: u32) -> (f32, f32) {
        let source = self
            .source
            .as_ref()
            .map_or(0..0, |(_, source)| source.clone());
        let run_end = self.x + self.advance;
        let clusters = self.clusters();
        let next = |point: u32| {
            clusters
                .iter()
                .find(|c| c.start >= point)
                .map_or(run_end, |c| self.x + c.x)
                .min(run_end)
        };
        let x_start = |point: u32| {
            clusters
                .iter()
                .find(|c| c.start <= point && point < c.end)
                .map_or_else(|| next(point), |c| (self.x + c.x).min(run_end))
        };
        let left = x_start(start.max(source.start));
        let right = if start == end {
            left
        } else {
            next(end.min(source.end)).max(left)
        };
        (left, right)
    }
}

/// A cluster of a text run (a grapheme, or a ligature): the part of the
/// text node's data its characters are (UTF-16; white space that collapsed
/// away after it isn't), and where it starts from the run's start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextCluster {
    pub start: u32,
    pub end: u32,
    pub x: f32,
}

/// The boxes CSS treats specially, for example when propagating a
/// background to the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxRole {
    LayoutView,
    /// A block box around inline content next to block siblings
    /// (css-display's anonymous block box).
    AnonymousBlock,
    /// A table, row group, row or cell that the table fixup made around
    /// content missing one (css-tables 3, "Fixup"; Blink's anonymous
    /// `LayoutTable` and its parts): its style's `display` says which.
    AnonymousTable,
    RootElement,
    /// "The HTML `body` element": the root is an HTML `html` element and
    /// this is its first `body` child.
    Body,
    Other,
}

/// An out-of-flow box where it is in tree order, among its parent's
/// children.
#[derive(Debug, Clone, PartialEq)]
pub struct OutOfFlowPlace {
    /// The parent's child it comes before (in tree order), the number of
    /// children if none.
    pub before: usize,
    /// Its fragment, its containing block's child.
    pub fragment: Arc<BoxFragment>,
}

/// A child fragment and where it sits (`blink::PhysicalFragmentLink`).
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentLink {
    /// From the parent's border-box origin to the child's.
    pub offset: PointF,
    pub fragment: Arc<BoxFragment>,
}

impl BoxFragment {
    /// A text dump of the tree, one fragment per line: node id, offset
    /// from the parent, size. For tests and debugging.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_into(&mut out, PointF::new(0.0, 0.0), 0);
        out
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "depth is the fragment's in the tree, far below usize::MAX / 2"
    )]
    fn dump_into(&self, out: &mut String, offset: PointF, depth: usize) {
        let name = match (self.node, self.role) {
            (Some(id), _) => format!("box #{}", id.get()),
            (None, BoxRole::AnonymousBlock) => "anonymous block".to_owned(),
            (None, BoxRole::AnonymousTable) => "anonymous table box".to_owned(),
            (None, _) => "LayoutView".to_owned(),
        };
        let _ = writeln!(
            out,
            "{:indent$}{name} at ({},{}) size {}x{}",
            "",
            offset.x,
            offset.y,
            self.size.width,
            self.size.height,
            indent = depth * 2
        );
        for line in &self.lines {
            let r = line.rect;
            let _ = writeln!(
                out,
                "{:indent$}line at ({},{}) size {}x{}: {} runs",
                "",
                r.x,
                r.y,
                r.width,
                r.height,
                line.runs.len(),
                indent = (depth + 1) * 2
            );
            for b in &line.boxes {
                let r = b.rect;
                let _ = writeln!(
                    out,
                    "{:indent$}inline box #{} at ({},{}) size {}x{}",
                    "",
                    b.node.get(),
                    r.x,
                    r.y,
                    r.width,
                    r.height,
                    indent = (depth + 2) * 2
                );
            }
        }
        for child in &self.children {
            child.fragment.dump_into(out, child.offset, depth + 1);
        }
    }
}
