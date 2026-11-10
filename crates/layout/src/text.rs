//! Inline formatting contexts: collapsing white space, shaping and line
//! breaking with parley, and turning the lines into text and inline box
//! fragments.
//!
//! An inline element's start and end edges (margin, border, padding) are
//! 0px-high parley inline boxes kept with the content next to them
//! (`InlineBoxKind::StartEdge` and `EndEdge`, our change on the `machopo`
//! branch of our parley fork), so the edges take room on the line without
//! changing its height. Each line then gets
//! one `InlineBoxFragment` per element on it, sliced at line breaks
//! (`box-decoration-break: slice`).
//!
//! An atomic inline (an `inline-block`, an `<img>`) is a box of the box
//! tree, which lays it out when the lines are broken ([`AtomicInlines`]);
//! here it is a parley inline box of its margin box's size, on the
//! baseline.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};

use computed_style::{
    ComputedStyle, Display, FontStyle, Hyphens, LengthPercentage, LengthPercentageOrAuto,
    LineHeight, OverflowWrap, Position, PseudoId, Sides, TextAlign, TextTransform,
    WhiteSpaceCollapse, WordBreak,
};
use gfx::NodeId;
use gfx::{FontData, PointF, RectF, SizeF};
use icu_segmenter::WordSegmenter;
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{
    Alignment, AlignmentOptions, FontContext, IndentOptions, InlineBox, InlineBoxKind, Layout,
    LayoutContext, PositionedLayoutItem, StyleProperty, TextStyle, TextWrapMode, TreeBuilder,
    VerticalAlign, YieldData,
};

use taffy::AvailableSpace;

use crate::fragment::{
    AtomicInline, InlineBoxFragment, LineEnd, LineEndAt, LineFragment, ShapedSpan, TextCluster,
    TextEnd, TextFragment, TextGlyphs,
};

static AHEM: &[u8] = include_bytes!("../../../resources/test_fonts/Ahem.ttf");
static LATO: &[u8] = include_bytes!("../../../resources/test_fonts/Lato-Medium.ttf");

/// Fonts and parley's scratch space, one per renderer main thread.
pub struct TextContext {
    fonts: FontContext,
    layout: LayoutContext<u32>,
    /// Font family names given parley since [`Self::take_family_names`]
    /// (`BoxTreeUpdate`'s `font_family_names`).
    family_names: usize,
}

impl TextContext {
    /// `test_fonts`: only the built-in Ahem and Lato, with every generic
    /// family mapped to Lato, so text renders the same on every machine.
    /// Otherwise the system's fonts.
    pub fn new(test_fonts: bool) -> Self {
        let fonts = if test_fonts {
            let mut collection = Collection::new(CollectionOptions {
                shared: false,
                system_fonts: false,
            });
            collection.register_fonts(Blob::new(Arc::new(AHEM)), None);
            let lato = collection.register_fonts(Blob::new(Arc::new(LATO)), None);
            let lato = lato.first().expect("Lato registers one family").0;
            for &generic in parley::GenericFamily::all() {
                collection.set_generic_families(generic, std::iter::once(lato));
            }
            FontContext {
                collection,
                source_cache: SourceCache::default(),
            }
        } else {
            FontContext::new()
        };
        let mut layout = LayoutContext::new();
        // Nothing moves through this text by words with parley's clusters:
        // editing and `text-transform: capitalize` find words themselves.
        layout.set_word_boundaries(false);
        Self {
            fonts,
            layout,
            family_names: 0,
        }
    }

    /// The font family names given parley since the last call.
    pub(crate) fn take_family_names(&mut self) -> usize {
        std::mem::take(&mut self.family_names)
    }

    /// How many layouts took the shaping of one alike (parley keeps what
    /// shaping a text gave), for tests.
    pub fn shapings_reused_for_testing(&self) -> usize {
        self.layout.shapings_reused()
    }

    /// How many words were taken from those shaped before (parley keeps
    /// what shaping a word gave, with fonts that shape a word the same on
    /// its own), and how many were shaped and kept, for tests.
    pub fn words_reused_and_shaped_for_testing(&self) -> (usize, usize) {
        self.layout.words_reused_and_shaped()
    }

    /// How many times two words were shaped again as one, as the font
    /// shapes a space together with the glyph beside it, for tests.
    pub fn words_joined_for_testing(&self) -> usize {
        self.layout.words_joined()
    }

    /// How many texts had their line break opportunities looked up in a
    /// table (parley does so for characters up to U+017F and from U+2000
    /// to U+203F, under the default rules), and how many of their words
    /// the table couldn't decide and were given to ICU4X, for tests.
    pub fn line_breaks_looked_up_for_testing(&self) -> (usize, usize) {
        self.layout.line_breaks_looked_up()
    }

    /// Adds the fonts in `data` (a font file), for tests that need fonts
    /// with features or axes the built-in ones lack: the names of the
    /// families it holds, which `font-family` picks them by.
    pub fn register_fonts_for_testing(&mut self, data: Vec<u8>) -> Vec<String> {
        let families = self
            .fonts
            .collection
            .register_fonts(Blob::new(Arc::new(data)), None);
        // Font selection may pick the new fonts for text shaped before.
        self.layout.clear_shapings();
        families
            .into_iter()
            .filter_map(|(id, _)| self.fonts.collection.family_name(id).map(str::to_owned))
            .collect()
    }

    /// Its font and layout contexts, for a layout of its own (a canvas's
    /// text).
    pub(crate) fn contexts(
        &mut self,
    ) -> (&mut parley::FontContext, &mut parley::LayoutContext<u32>) {
        (&mut self.fonts, &mut self.layout)
    }
}

/// How far past the width a line may run before it breaks.
const BREAK_TOLERANCE: f32 = 1.0 / 64.0;

/// U+FFFC OBJECT REPLACEMENT CHARACTER: an atomic inline in the text.
const OBJECT_REPLACEMENT: char = '\u{FFFC}';

/// Whether a bold weight landed on a face that isn't bold, so the glyphs
/// need emboldening. fontique only asks for it when the requested weight is
/// more than 200 above the face's (Blink's rule in `font_cache_skia.cc`),
/// which leaves `bold` (700) on a Medium (500) face thin. Blink's fallback
/// typefaces (`CreateFontPlatformDataForTypeface`) embolden whenever the
/// weight reaches `kBoldThreshold` (600) and the face is lighter, and so does
/// Firefox; Blink on macOS (`font_cache_mac.mm`) does from 501 on, for faces
/// without the bold trait. A face with a `wght` axis gets the weight from the
/// axis instead.
pub(crate) fn needs_synthetic_bold(requested: f32, font: &parley::FontData) -> bool {
    use read_fonts::types::Tag;
    use read_fonts::{FontRef, TableProvider};

    const BOLD_THRESHOLD: f32 = 600.0;
    if requested < BOLD_THRESHOLD {
        return false;
    }
    let Ok(face) = FontRef::from_index(font.data.data(), font.index) else {
        return false;
    };
    let variable = face
        .fvar()
        .and_then(|fvar| fvar.axes())
        .is_ok_and(|axes| axes.iter().any(|axis| axis.axis_tag() == Tag::new(b"wght")));
    // fontique's default when a face has no OS/2 table.
    let weight = face.os2().map_or(400, |os2| os2.us_weight_class());
    !variable && f32::from(weight) < BOLD_THRESHOLD
}

/// `text` shaped as one line, without wrapping: SVG's text, which its
/// own layout places character by character. Each span is a range of
/// `text` in a style, its index the parley brush.
pub(crate) fn shape_line(
    cx: &mut TextContext,
    text: &str,
    spans: &[(std::ops::Range<usize>, &ComputedStyle)],
) -> Layout<u32> {
    let mut builder = cx.layout.ranged_builder(&mut cx.fonts, text, 1.0, true);
    for (index, (range, style)) in spans.iter().enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a span per run of one element's text: fewer than 2^32"
        )]
        let brush = index as u32;
        for property in text_properties(style, brush, BlockText::default()) {
            builder.push(property, range.clone());
        }
        builder.push(
            StyleProperty::TextWrapMode(TextWrapMode::NoWrap),
            range.clone(),
        );
    }
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout
}

/// The ellipsis for `style`'s lines (Blink's `LineTruncator::SetupEllipsis`):
/// U+2026 in the block's style, or "..." when its first font has no glyph
/// for it (the font parley shapes it in isn't the one it shapes a Latin
/// letter in), at x 0 on baseline 0. It is no text node's: it is placed
/// and attributed to an element line by line.
fn ellipsis_fragment(cx: &mut TextContext, style: &Arc<ComputedStyle>) -> TextFragment {
    substitute_fragment(cx, style, "\u{2026}", "...")
}

/// The hyphen a line broken after a soft hyphen ends with, in the soft
/// hyphen's style (Blink's `ComputedStyle::HyphenString`): U+2010, or "-"
/// when its first font has no glyph for it, at x 0 on baseline 0.
fn hyphen_fragment(cx: &mut TextContext, style: &Arc<ComputedStyle>) -> TextFragment {
    substitute_fragment(cx, style, "\u{2010}", "-")
}

/// `preferred` shaped in `style`, or `fallback` when the font parley shapes
/// it in isn't the one it shapes a Latin letter in, at x 0 on baseline 0.
fn substitute_fragment(
    cx: &mut TextContext,
    style: &Arc<ComputedStyle>,
    preferred: &'static str,
    fallback: &'static str,
) -> TextFragment {
    let first_font = |cx: &mut TextContext, text: &str| {
        let layout = shape_line(cx, text, &[(0..text.len(), style)]);
        let run = layout.lines().next()?.runs().next()?;
        let font = run.font();
        Some((font.data.id(), font.index))
    };
    let text = if first_font(cx, preferred) == first_font(cx, "X") {
        preferred
    } else {
        fallback
    };
    let layout = shape_line(cx, text, &[(0..text.len(), style)]);
    let mut glyphs = Vec::new();
    let mut fragment = TextFragment {
        node: None,
        pseudo: None,
        source: None,
        glyphs: TextGlyphs::Owned(Arc::default()),
        font: FontData {
            bytes: Arc::new(Blob::new(Arc::new([]))),
            index: 0,
            id: 0,
        },
        font_size: style.font.size,
        embolden: false,
        skew: 0.0,
        variations: Arc::new([]),
        x: 0.0,
        advance: 0.0,
        hidden_from: None,
        visible_advance: 0.0,
        baseline: 0.0,
        ascent: 0.0,
        descent: 0.0,
        underline_offset: 0.0,
        underline_size: 0.0,
        strikethrough_offset: 0.0,
        strikethrough_size: 0.0,
        style: style.clone(),
    };
    for line in layout.lines() {
        let line_baseline = line.metrics().baseline;
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let run_metrics = run.font_metrics();
            let synthesis = run.synthesis();
            if glyphs.is_empty() {
                fragment.font = FontData {
                    bytes: Arc::new(font.data.clone()),
                    index: font.index,
                    id: font.data.id(),
                };
                fragment.font_size = run.font_size();
                fragment.embolden = crate::font_features::embolden(
                    &style.font,
                    synthesis.embolden() || needs_synthetic_bold(style.font.weight, font),
                );
                fragment.skew = crate::font_features::skew(&style.font, synthesis.skew());
                fragment.variations = crate::font_features::run_variations(run, Some(&style.font));
                fragment.ascent = run_metrics.ascent;
                fragment.descent = run_metrics.descent;
                fragment.underline_offset = run_metrics.underline_offset;
                fragment.underline_size = run_metrics.underline_size;
                fragment.strikethrough_offset = run_metrics.strikethrough_offset;
                fragment.strikethrough_size = run_metrics.strikethrough_size;
            }
            glyphs.extend(
                glyph_run
                    .positioned_glyphs()
                    .map(|g| (g.id, PointF::new(g.x, g.y - line_baseline))),
            );
            fragment.advance = glyph_run.offset() + glyph_run.advance();
        }
    }
    fragment.glyphs = TextGlyphs::Owned(glyphs.into());
    fragment.visible_advance = fragment.advance;
    fragment
}

/// Whether `glyph_run` has the cluster that ends at `end` in the text: the
/// last on a line that ends there.
fn ends_with(glyph_run: &parley::GlyphRun<'_, u32>, end: usize) -> bool {
    let style = glyph_run.style_index();
    glyph_run
        .run()
        .visual_clusters()
        .any(|cluster| cluster.style_index() == style && cluster.text_range().end == end)
}

/// A line's in-flow child, for `text-overflow`: a run or an atomic inline,
/// by its index in the line.
#[derive(Clone, Copy)]
enum LineChild {
    Run(usize),
    Atomic(usize),
}

/// Ends a line that overflows its block's content box (whose right edge
/// is `right`) with `ellipsis` (Blink's `LineTruncator::TruncateLine`):
/// from the last child back, one wholly outside the box is hidden; the
/// first with room for the ellipsis after it is where the ellipsis goes,
/// a run cut at the last cluster that leaves that room, and the rest
/// hidden. The first child keeps at least one cluster, or an atomic inline
/// whole: the first character is clipped, not ellipsed (css-overflow 4).
/// The ellipsis goes after the runs (painted over them) and is attributed
/// to the child it follows, so it is hit as that text is. Adds the glyphs
/// it places to `glyphs`.
fn truncate_line(
    ellipsis: &TextFragment,
    right: f32,
    baseline: f32,
    children: &[(LineChild, f32, f32)],
    runs: &mut Vec<TextFragment>,
    atomic_inlines: &mut [AtomicInline],
    glyphs: &mut usize,
) {
    let mut hide = |runs: &mut Vec<TextFragment>, child: LineChild| match child {
        LineChild::Run(run) => {
            runs[run].hidden_from = Some(0);
            runs[run].visible_advance = 0.0;
        }
        LineChild::Atomic(atomic) => atomic_inlines[atomic].hidden = true,
    };
    let mut placed = None;
    for (index, &(child, left, child_right)) in children.iter().enumerate().rev() {
        let first = index == 0;
        let mut space = right - left;
        if space <= 0.0 {
            if !first {
                hide(runs, child);
            }
            continue;
        }
        space -= ellipsis.advance;
        // What fits exactly fits, rounding aside.
        if space + BREAK_TOLERANCE >= child_right - left {
            placed = Some((child, child_right));
            break;
        }
        let run = match child {
            LineChild::Run(run) => run,
            LineChild::Atomic(_) => {
                if first {
                    placed = Some((child, child_right));
                    break;
                }
                hide(runs, child);
                continue;
            }
        };
        // Where each cluster starts (each ends where the next starts).
        let fragment = &runs[run];
        let starts: Vec<f32> = if fragment.clusters().is_empty() {
            let starts: Vec<f32> = fragment.glyphs().map(|(_, p)| p.x).collect();
            *glyphs = glyphs.saturating_add(starts.len());
            starts
        } else {
            fragment
                .clusters()
                .iter()
                .map(|c| fragment.x + c.x)
                .collect()
        };
        let limit = left + space;
        let end = |n: usize| starts.get(n).copied().unwrap_or(child_right);
        // The clusters end further right one after another: the most that
        // fit are the first from the end that do.
        let mut fit = (1..=starts.len())
            .rev()
            .find(|&n| end(n) <= limit)
            .unwrap_or(0);
        if fit == 0 || fit == starts.len() {
            if !first {
                hide(runs, child);
                continue;
            }
            fit = fit.checked_sub(1).unwrap_or(1);
        }
        let cut = end(fit);
        let fragment = &mut runs[run];
        let xs: Vec<f32> = fragment.glyphs().map(|(_, p)| p.x).collect();
        *glyphs = glyphs.saturating_add(xs.len());
        fragment.hidden_from = Some(xs.iter().filter(|&&x| x < cut).count());
        fragment.visible_advance = cut - fragment.x;
        placed = Some((child, cut));
        break;
    }
    let Some((child, x)) = placed else {
        return;
    };
    let mut ellipsis = ellipsis.clone();
    ellipsis.translate(PointF::new(x, baseline));
    (ellipsis.node, ellipsis.pseudo) = match child {
        LineChild::Run(run) => (runs[run].node, runs[run].pseudo),
        // An atomic inline's element isn't known here: the block's.
        LineChild::Atomic(_) => (None, None),
    };
    runs.push(ellipsis);
}

/// Text clusters and glyphs that layout placed (`LayoutUpdate`): only
/// `text-overflow` needs them; the rest are placed when asked.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TextPlaced {
    pub(crate) clusters: usize,
    pub(crate) glyphs: usize,
}

/// Inline content in DOM order, before white space processing (Blink's
/// `InlineItem`).
pub(crate) enum InlineItem {
    /// A text node's data, with its style, what it is in, and the text node
    /// itself (`None` for text no node has: a pseudo-element's).
    Text(String, Arc<ComputedStyle>, Option<Owner>, Option<NodeId>),
    /// `<br>`, with its element.
    Break(Arc<ComputedStyle>, NodeId),
    /// Where an inline element (or pseudo-element) starts (`kOpenTag`).
    Open(Owner, Arc<ComputedStyle>),
    /// Where an inline element (or pseudo-element) ends (`kCloseTag`).
    Close(Owner, Arc<ComputedStyle>),
    /// An atomic inline: its box in the box tree (Blink's `kAtomicInline`),
    /// and its style.
    Atomic(taffy::NodeId, Arc<ComputedStyle>),
    /// An out-of-flow box, laid out by its containing block, whose static
    /// position is here (Blink's `kOutOfFlowPositioned`): its box and its
    /// style. White space collapses across it as if it weren't there.
    OutOfFlow(taffy::NodeId, Arc<ComputedStyle>),
    /// A float, placed beside the line it comes in, or below it if it
    /// doesn't fit there (Blink's `kFloating`): its box. White space
    /// collapses across it as across an out-of-flow box.
    Float(taffy::NodeId),
}

/// An atomic inline as the box tree laid it out.
pub(crate) struct AtomicLayout {
    pub(crate) margin_box: SizeF,
    /// From the margin box's top; `None` for its bottom edge.
    pub(crate) baseline: Option<f32>,
}

/// What an inline formatting context needs of the box tree: its atomic
/// inlines and floats laid out (Blink's `BlockNode::LayoutAtomicInline`),
/// and its floats placed (Blink's `ExclusionSpace`, which `LineBreaker::
/// HandleFloat` positions them in).
pub(crate) trait AtomicInlines {
    /// Counts the lines broken once more ([`crate::LayoutUpdate`]'s
    /// `lines_broken`).
    fn count_line_break(&mut self);

    /// Counts a line beside floats broken again from its start
    /// ([`crate::LayoutUpdate`]'s `lines_broken_again_beside_floats`).
    fn count_line_broken_again(&mut self);

    /// Lays atomic inline `id` out in `space`: the containing block's
    /// width, or none for a max-content size; `height` is the containing
    /// block's height, if definite.
    fn lay_out(
        &mut self,
        id: taffy::NodeId,
        space: AvailableSpace,
        height: Option<f32>,
    ) -> AtomicLayout;

    /// Atomic inline `id`'s min-content or max-content contribution: its
    /// margin box's width, only measured.
    fn width(&mut self, id: taffy::NodeId, space: AvailableSpace) -> f32;

    /// What `floats` add to the content's min-content or max-content
    /// width: the widest, or those beside each other side by side.
    fn float_widths(&mut self, floats: &[taffy::NodeId], space: AvailableSpace) -> f32;

    /// Whether the floats are placed as the lines are broken: there is a
    /// block formatting context to place them in. Otherwise they stay
    /// where they were placed last, beside the lines broken again.
    fn places_floats(&self) -> bool {
        false
    }

    /// Places float `id`, whose margin box is `size`, no higher than
    /// `top`: where its margin box goes. Both from the content box.
    fn place_float(&mut self, _id: taffy::NodeId, _size: SizeF, _top: f32) -> PointF {
        unreachable!("only where floats are placed")
    }

    /// The room the floats leave beside the lines now, once one more was
    /// placed.
    fn float_bands(&self) -> FloatBands {
        unreachable!("only where floats are placed")
    }
}

impl InlineItem {
    /// Whether white space processing would leave anything: an anonymous
    /// block is only generated around content that is.
    pub(crate) fn is_collapsible_space_only(items: &[InlineItem]) -> bool {
        items.iter().all(|item| match item {
            InlineItem::Text(text, style, ..) => {
                style.white_space_collapse.collapses_spaces()
                    && !style.white_space_collapse.preserves_newlines()
                    && text.chars().all(is_space)
            }
            InlineItem::Break(..)
            | InlineItem::Atomic(..)
            | InlineItem::OutOfFlow(..)
            | InlineItem::Float(_) => false,
            InlineItem::Open(..) | InlineItem::Close(..) => true,
        })
    }

    /// Whether it is only white space, whatever `white-space` says: a flex
    /// container makes no item around it (css-flexbox 1, "Flex Items"),
    /// even where `white-space: pre` would keep it.
    pub(crate) fn is_white_space_only(items: &[InlineItem]) -> bool {
        items.iter().all(|item| match item {
            InlineItem::Text(text, ..) => text.chars().all(is_space),
            InlineItem::Break(..)
            | InlineItem::Atomic(..)
            | InlineItem::OutOfFlow(..)
            | InlineItem::Float(_) => false,
            InlineItem::Open(..) | InlineItem::Close(..) => true,
        })
    }
}

/// CSS's document white space: space, tab, and the segment breaks.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// The room floats leave beside the lines, from the top of the content
/// box down (Blink's layout opportunities, from its exclusion space): in
/// tree order of their tops, each band's top, and the left edge and width
/// of the room in it, in the content box's coordinates. A band goes down to
/// the next one's top; the last, below every float, goes on down. Empty
/// when no float is beside the content: all of its width, all the way.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FloatBands(pub(crate) Vec<FloatBand>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloatBand {
    pub(crate) top: f32,
    pub(crate) x: f32,
    pub(crate) width: f32,
}

impl FloatBands {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The room for a line from `top` down `height` (0: where it starts),
    /// in a content box `width` wide: its left edge and width, what every
    /// band it reaches leaves.
    fn room(&self, top: f32, height: f32, width: f32) -> (f32, f32) {
        let (mut left, mut right) = (0.0_f32, width);
        let mut any = false;
        for (i, band) in self.0.iter().enumerate() {
            #[expect(clippy::arithmetic_side_effects, reason = "i < self.0.len(), a Vec's")]
            let end = self.0.get(i + 1).map_or(f32::INFINITY, |next| next.top);
            let reached = if height > 0.0 {
                band.top < top + height && end > top
            } else {
                band.top <= top && end > top
            };
            if reached {
                any = true;
                left = left.max(band.x);
                right = right.min(band.x + band.width);
            }
        }
        if !any {
            return (0.0, width);
        }
        (left, (right - left).max(0.0))
    }

    /// The first band's top below `top`: where the room changes next.
    fn next_top(&self, top: f32) -> Option<f32> {
        self.0.iter().map(|band| band.top).find(|&t| t > top)
    }
}

/// An inline formatting context's text, shaped and ready to break into
/// lines at any width.
pub(crate) struct InlineContent {
    /// The style and element of each run, indexed by the parley brush.
    styles: Vec<(Arc<ComputedStyle>, Option<Owner>)>,
    /// Where each run's text is in its text node, by the parley brush
    /// (Blink's `OffsetMapping`): for the boxes of a part of a text node.
    sources: Vec<Option<Arc<TextSource>>>,
    /// Where each `<br>`'s newline is in the processed text (bytes), the
    /// parley brush of its run, and the `<br>`: a line ending in one ends
    /// in a `<br>`, not in a newline of its text.
    breaks: Vec<(u32, u32, NodeId)>,
    layout: Layout<u32>,
    text_align: TextAlign,
    /// The containing block's `text-indent`, resolved when the lines are
    /// broken (a percentage of the width they are broken at).
    text_indent: LengthPercentage,
    /// The inline elements with an edge here, in tree order.
    elements: Vec<InlineElement>,
    /// What each parley inline box is, by its id.
    boxes: Vec<BoxItem>,
    /// The atomic inlines' boxes, in order.
    atomics: Vec<taffy::NodeId>,
    /// The out-of-flow boxes whose static position is here, in order, with
    /// the `display` they had before they were blockified.
    out_of_flow: Vec<(taffy::NodeId, Display)>,
    /// The floats here, in order, with their margin boxes as last laid
    /// out and where the lines last placed them, from the content box.
    floats: Vec<InlineFloat>,
    /// Elements that started before this content (a block inside them
    /// split them), outermost first.
    open_at_start: Vec<usize>,
    /// What percentages in the edges resolve against: the containing
    /// block's width the boxes were last sized in, or 0 with none.
    percent_base: f32,
    /// The space the parley boxes were last sized in; `None` once an
    /// atomic inline changed.
    sized_for: Option<AvailableSpace>,
    /// The containing block's height for the atomic inlines' percentages,
    /// if definite: the block's content box's.
    height: Option<f32>,
    /// The width and the floats the lines were last broken beside, and the
    /// size they took: breaking beside them again changes nothing while the
    /// boxes keep their sizes.
    broken: Option<(Option<f32>, FloatBands, SizeF)>,
    /// The floats beside the lines when they were last laid out for real:
    /// what they are broken beside again for their fragments, once taffy's
    /// block formatting context is gone.
    laid_out_beside: FloatBands,
    /// The narrowest and widest the content can be, once asked: flex and
    /// the intrinsic size keywords ask often. Only an atomic inline
    /// changes it in place (a new style in place is only one that changes
    /// what is painted).
    content_widths: Option<(f32, f32)>,
    /// The ellipsis `text-overflow` ends an overflowing line with, and a
    /// line clamp its last line, shaped in the block's style, at x 0 on
    /// baseline 0; `None` when the block does neither.
    ellipsis: Option<TextFragment>,
    /// The hyphen a line broken after a soft hyphen ends with, by the
    /// parley brush of the soft hyphen's run, shaped in its style at x 0 on
    /// baseline 0: for each run with a soft hyphen whose `hyphens` isn't
    /// `none`; empty without any.
    hyphens: Vec<Option<TextFragment>>,
    /// Whether `text-overflow` truncates the lines that overflow.
    truncates: bool,
    /// The line a line clamp ends with an ellipsis: the lines after it are
    /// hidden, laid out but not painted or hit (Blink's line clamp's
    /// `IsHiddenForPaint`). Set by the clamp container's layout.
    clamp: Option<usize>,
}

struct InlineElement {
    node: Owner,
    style: Arc<ComputedStyle>,
}

/// What an inline box or a run is of: an element, or one of its
/// pseudo-elements, whose boxes hit testing finds as the element's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Owner {
    pub(crate) node: NodeId,
    pub(crate) pseudo: Option<PseudoId>,
}

impl Owner {
    pub(crate) fn element(node: NodeId) -> Self {
        Self { node, pseudo: None }
    }
}

struct InlineFloat {
    id: taffy::NodeId,
    margin_box: SizeF,
    placed: PointF,
}

enum BoxItem {
    /// An element's start or end edge.
    Edge { element: usize, start: bool },
    /// An atomic inline, by its index in `atomics`: its margin box is the
    /// parley box.
    Atomic(usize),
    /// An out-of-flow box, by its index in `out_of_flow`: parley places it
    /// as an empty box that takes no room.
    OutOfFlow(usize),
    /// A float, by its index in `floats`: parley hands it to the line
    /// breaking, which places it.
    Float(usize),
}

/// What an inline formatting context passes up to the containing blocks of
/// its out-of-flow boxes, in tree order: each out-of-flow box here, and
/// those an atomic inline passed up.
pub(crate) enum PassedUp {
    /// By its index in [`InlineContent::out_of_flow`].
    OutOfFlow(usize),
    /// By its index in [`InlineContent::atomics`].
    Atomic(usize),
    /// By its index in [`InlineContent::floats`].
    Float(usize),
}

impl InlineContent {
    /// Processes white space (css-text phase I, across element boundaries
    /// like Blink's `InlineItemsBuilder`) and shapes the text. `None` if
    /// nothing is left. `container` is the style of the block the content
    /// is in. In a text control's inner editor (`editor`), a forced break
    /// ending the content is followed by an empty line, where the caret
    /// goes (Blink adds a placeholder `<br>` to such a value); anywhere
    /// else no line follows it (Blink's `LineBreaker` stops at the end of
    /// the items), so `<div>a<br></div>` is one line tall. `truncates`
    /// if `text-overflow` truncates the lines, `clamped` if a line clamp
    /// may end one: both need the ellipsis.
    pub(crate) fn new(
        cx: &mut TextContext,
        container: &Arc<ComputedStyle>,
        items: &[InlineItem],
        truncates: bool,
        clamped: bool,
        editor: bool,
    ) -> Option<Self> {
        let (text, runs, tags) = collapse_white_space(items)?;
        // A `<br>`'s run: of no text node, in no element (a pseudo-
        // element's is in its element); one each, in order.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a byte offset into the text, and a run of it: past u32 only past 4 GB of text"
        )]
        let breaks: Vec<(u32, u32, NodeId)> = runs
            .iter()
            .enumerate()
            .filter(|(_, (_, _, node, source))| node.is_none() && source.is_none())
            .map(|(brush, (range, ..))| (range.start as u32, brush as u32))
            .zip(items.iter().filter_map(|item| match item {
                InlineItem::Break(_, br) => Some(*br),
                _ => None,
            }))
            .map(|((at, brush), br)| (at, brush, br))
            .collect();
        let ellipsis = (truncates || clamped).then(|| ellipsis_fragment(cx, container));

        // The inline elements with an edge here, in tree order; those
        // closed here without opening were open before this content (a
        // block inside them split them), outermost first.
        let mut elements: Vec<InlineElement> = Vec::new();
        let mut open_at_start = Vec::new();
        for &(_, item) in &tags {
            let (node, style, start) = match &items[item] {
                InlineItem::Open(node, style) => (*node, style, true),
                InlineItem::Close(node, style) => (*node, style, false),
                _ => continue,
            };
            if elements.iter().all(|e| e.node != node) {
                if !start {
                    // Inner elements close first, so each goes outside the
                    // ones so far.
                    open_at_start.insert(0, elements.len());
                }
                elements.push(InlineElement {
                    node,
                    style: style.clone(),
                });
            }
        }
        // `vertical-align` as parley has it, resolved before the builder
        // borrows `cx` (a percentage of a `normal` line height needs the
        // font): each inline element's and each atomic inline's. The
        // block's own text has none: a block's `vertical-align` (an
        // absolutely positioned span's) means nothing to its lines.
        let element_aligns: Vec<VerticalAlign> = elements
            .iter()
            .map(|element| vertical_align(cx, &element.style))
            .collect();
        let atomic_aligns: Vec<VerticalAlign> = tags
            .iter()
            .filter_map(|&(_, item)| match &items[item] {
                InlineItem::Atomic(_, style) => Some(vertical_align(cx, style)),
                _ => None,
            })
            .collect();

        // parley's tree of spans, as Blink's inline items nest: a span per
        // inline element inside its parent's (its `vertical-align` is
        // relative to the parent), a span per run inside its element's
        // (the run's brush, no shift of its own), and the edges and atomic
        // inlines as inline boxes where they are. The white space was
        // processed already: every span keeps what it gets
        // (`WhiteSpaceCollapse::Preserve`), and the text comes back as it
        // went in.
        // Tab stops, when there are tabs to place: they are counted in the
        // block's space.
        let tabs = text.contains('\t').then(|| BlockTabs::new(cx, container));
        // The hyphens, when there are soft hyphens: parley takes their
        // advance with the style of each soft hyphen's run (an element's
        // or the block's when the run has it).
        let hyphens: Vec<Option<TextFragment>> = if text.contains('\u{AD}') {
            runs.iter()
                .map(|(range, style, ..)| {
                    (text[range.clone()].contains('\u{AD}')
                        && style.rare_inherited.hyphens != Hyphens::None)
                        .then(|| hyphen_fragment(cx, style))
                })
                .collect()
        } else {
            Vec::new()
        };
        let hyphen_advances: Vec<(*const ComputedStyle, f32)> = runs
            .iter()
            .zip(&hyphens)
            .filter_map(|((_, style, ..), hyphen)| {
                Some((Arc::as_ptr(style), hyphen.as_ref()?.advance))
            })
            .collect();
        let block = BlockText {
            tabs,
            hyphens: &hyphen_advances,
        };
        let root = text_style(container, 0, block);
        let mut names = family_names(&root.font_family);
        let mut builder = cx.layout.tree_builder(&mut cx.fonts, 1.0, true, &root);
        let element_span =
            |builder: &mut TreeBuilder<'_, u32>, names: &mut usize, element: usize| {
                let mut properties = text_properties(&elements[element].style, 0, block);
                *names = names.wrapping_add(property_family_names(&properties));
                properties.push(StyleProperty::VerticalAlign(element_aligns[element]));
                builder.push_style_modification_span(&properties);
            };
        for &element in &open_at_start {
            element_span(&mut builder, &mut names, element);
        }
        // The elements whose spans are open, innermost last: a run whose
        // style isn't the innermost's (text in a `display: contents`
        // element, which has no span) takes its own text properties.
        let mut open = open_at_start.clone();
        let mut styles = Vec::with_capacity(runs.len());
        let mut sources = Vec::with_capacity(runs.len());
        let mut runs: VecDeque<(usize, RunItem<'_>)> = runs.into_iter().enumerate().collect();
        // The edges, as boxes kept with the element's first and last
        // content, and the atomic inlines. Their sizes are set when the
        // lines are broken, once percentages have something to resolve
        // against.
        let mut boxes = Vec::new();
        let mut atomics = Vec::new();
        let mut out_of_flow = Vec::new();
        let mut floats = Vec::new();
        for &(byte_index, item) in &tags {
            let enclosing = open.last().map_or(&**container, |&e| &*elements[e].style);
            push_runs(
                &mut builder,
                &text,
                &mut runs,
                byte_index,
                enclosing,
                block,
                &mut styles,
                &mut sources,
                &mut names,
            );
            let (element, start) = match &items[item] {
                InlineItem::Open(node, _) | InlineItem::Close(node, _) => (
                    elements
                        .iter()
                        .position(|e| e.node == *node)
                        .expect("listed above"),
                    matches!(items[item], InlineItem::Open(..)),
                ),
                InlineItem::Atomic(id, _) => {
                    builder.push_inline_box(InlineBox {
                        id: boxes.len() as u64,
                        kind: InlineBoxKind::InFlow,
                        index: byte_index,
                        width: 0.0,
                        height: 0.0,
                        baseline: None,
                        vertical_align: atomic_aligns[atomics.len()],
                    });
                    boxes.push(BoxItem::Atomic(atomics.len()));
                    atomics.push(*id);
                    continue;
                }
                InlineItem::OutOfFlow(id, style) => {
                    builder.push_inline_box(InlineBox {
                        id: boxes.len() as u64,
                        kind: InlineBoxKind::OutOfFlow,
                        index: byte_index,
                        width: 0.0,
                        height: 0.0,
                        baseline: None,
                        vertical_align: VerticalAlign::BASELINE,
                    });
                    boxes.push(BoxItem::OutOfFlow(out_of_flow.len()));
                    out_of_flow.push((*id, style.original_display));
                    continue;
                }
                InlineItem::Float(id) => {
                    builder.push_inline_box(InlineBox {
                        id: boxes.len() as u64,
                        kind: InlineBoxKind::CustomOutOfFlow,
                        index: byte_index,
                        width: 0.0,
                        height: 0.0,
                        baseline: None,
                        vertical_align: VerticalAlign::BASELINE,
                    });
                    boxes.push(BoxItem::Float(floats.len()));
                    floats.push(InlineFloat {
                        id: *id,
                        margin_box: SizeF::new(0.0, 0.0),
                        placed: PointF::new(0.0, 0.0),
                    });
                    continue;
                }
                _ => unreachable!("only tags and atomic inlines are placed"),
            };
            // The element's span, its start edge inside it and its end
            // edge before it ends.
            if start {
                element_span(&mut builder, &mut names, element);
                open.push(element);
            } else {
                open.pop();
            }
            builder.push_inline_box(InlineBox {
                id: boxes.len() as u64,
                kind: if start {
                    InlineBoxKind::StartEdge
                } else {
                    InlineBoxKind::EndEdge
                },
                index: byte_index,
                width: 0.0,
                height: 0.0,
                baseline: None,
                vertical_align: VerticalAlign::BASELINE,
            });
            boxes.push(BoxItem::Edge { element, start });
            if !start {
                builder.pop_style_span();
            }
        }
        let enclosing = open.last().map_or(&**container, |&e| &*elements[e].style);
        push_runs(
            &mut builder,
            &text,
            &mut runs,
            usize::MAX,
            enclosing,
            block,
            &mut styles,
            &mut sources,
            &mut names,
        );
        let (mut layout, built) = builder.build();
        cx.family_names = cx.family_names.wrapping_add(names);
        debug_assert_eq!(built, text, "parley keeps the processed text");
        layout.set_line_after_final_break(editor);
        Some(Self {
            styles,
            sources,
            breaks,
            layout,
            text_align: container.text_align,
            text_indent: container.rare_inherited.text_indent,
            elements,
            boxes,
            atomics,
            out_of_flow,
            floats,
            open_at_start,
            percent_base: 0.0,
            sized_for: None,
            height: None,
            broken: None,
            laid_out_beside: FloatBands::default(),
            content_widths: None,
            ellipsis,
            hyphens,
            truncates,
            clamp: None,
        })
    }

    /// Leaves the first line unindented: these lines don't start their
    /// element's first formatted line (css-text 3), as an anonymous
    /// block's after a block box doesn't (Blink's `LayoutBlockFlow::
    /// CanContainFirstFormattedLine`).
    pub(crate) fn not_first_formatted_line(&mut self) {
        self.text_indent = LengthPercentage::Px(0.0);
    }

    /// The number of lines as broken last.
    pub(crate) fn line_count(&self) -> usize {
        self.layout.len()
    }

    /// The bottom of line `index` as broken last, from the content box's
    /// top.
    pub(crate) fn line_bottom(&self, index: usize) -> f32 {
        self.layout.get(index).map_or(0.0, |line| {
            let metrics = line.metrics();
            metrics.block_min_coord + metrics.line_height
        })
    }

    /// Ends the lines with line `index`, which a line clamp ends with an
    /// ellipsis, hiding those after it; `None` for none. Whether that
    /// changes anything.
    pub(crate) fn set_clamp(&mut self, index: Option<usize>) -> bool {
        std::mem::replace(&mut self.clamp, index) != index
    }

    /// The atomic inlines' boxes, in order.
    pub(crate) fn atomics(&self) -> &[taffy::NodeId] {
        &self.atomics
    }

    /// The out-of-flow boxes whose static position is here, in order.
    pub(crate) fn out_of_flow(&self) -> impl Iterator<Item = taffy::NodeId> + '_ {
        self.out_of_flow.iter().map(|&(id, _)| id)
    }

    /// The floats here, in order, each with where the lines last placed
    /// its margin box, from the content box.
    pub(crate) fn floats(&self) -> impl Iterator<Item = (taffy::NodeId, PointF)> + '_ {
        self.floats.iter().map(|f| (f.id, f.placed))
    }

    /// Where the floats placed with the lines last broken end, from the
    /// content box's top: what holds them, if it is their block
    /// formatting context's root, grows down to there.
    pub(crate) fn floats_bottom(&self) -> f32 {
        self.floats
            .iter()
            .map(|f| f.placed.y + f.margin_box.height)
            .fold(0.0, f32::max)
    }

    /// The out-of-flow boxes here, the atomic inlines and the floats, in
    /// tree order: what may pass out-of-flow boxes up.
    pub(crate) fn passed_up(&self) -> impl Iterator<Item = PassedUp> + '_ {
        self.boxes.iter().filter_map(|item| match *item {
            BoxItem::Edge { .. } => None,
            BoxItem::Atomic(index) => Some(PassedUp::Atomic(index)),
            BoxItem::OutOfFlow(index) => Some(PassedUp::OutOfFlow(index)),
            BoxItem::Float(index) => Some(PassedUp::Float(index)),
        })
    }

    /// The height its lines took the last time they were broken.
    pub(crate) fn lines_height(&self) -> Option<f32> {
        self.broken.as_ref().map(|(_, _, size)| size.height)
    }

    /// The block's content box is `height` high, if definite: what the
    /// atomic inlines' percentage heights are of (CSS 2 § 10.5).
    pub(crate) fn set_height(&mut self, height: Option<f32>) {
        if height != self.height {
            self.height = height;
            self.atomics_changed();
        }
    }

    /// The containing block's height for percentages, if definite.
    pub(crate) fn height(&self) -> Option<f32> {
        self.height
    }

    /// An atomic inline here changed: it is laid out again for its size.
    pub(crate) fn atomics_changed(&mut self) {
        self.sized_for = None;
        self.content_widths = None;
    }

    /// The style an inline element here was shaped with: that of its
    /// edges, which every one with content here has, or a `<br>`'s, of its
    /// newline's run.
    pub(crate) fn style_of(&self, element: NodeId) -> Option<&Arc<ComputedStyle>> {
        if let Some(&(_, brush, _)) = self.breaks.iter().find(|&&(.., br)| br == element) {
            return Some(&self.styles[brush as usize].0);
        }
        let element = Owner::element(element);
        let edge = self.elements.iter().find(|e| e.node == element);
        edge.map(|e| &e.style)
    }

    /// Gives `element`'s text and edges here `style`, or a `<br>`'s
    /// newline's run, which differs from theirs only in what is painted:
    /// nothing is shaped again.
    pub(crate) fn set_style(&mut self, element: NodeId, style: &Arc<ComputedStyle>) {
        for &(_, brush, br) in &self.breaks {
            if br == element {
                self.styles[brush as usize].0 = style.clone();
            }
        }
        let element = Owner::element(element);
        for (run_style, node) in &mut self.styles {
            if *node == Some(element) {
                *run_style = style.clone();
            }
        }
        for e in &mut self.elements {
            if e.node == element {
                e.style = style.clone();
            }
        }
    }

    /// Sizes each parley box in `space`, the containing block's width or
    /// none: an edge's width, with percentages of that width (0 with
    /// none), or an atomic inline's margin box and baseline, as the box
    /// tree lays it out. The atomic inlines were last laid out in the
    /// space they were sized in, which the fragments are made from.
    fn resize_boxes(&mut self, space: AvailableSpace, atomics: &mut dyn AtomicInlines) {
        if self.sized_for == Some(space) {
            return;
        }
        self.sized_for = Some(space);
        self.percent_base = match space {
            AvailableSpace::Definite(width) => width,
            _ => 0.0,
        };
        let mut changed = false;
        let boxes = self.layout.inline_boxes().len();
        for index in 0..boxes {
            self.layout.edit_inline_box(index, |inline_box| {
                let (width, height, baseline) = match &self.boxes[box_index(inline_box.id)] {
                    BoxItem::Edge { element, start } => {
                        let edge = Edge::of(&self.elements[*element].style, self.percent_base);
                        (if *start { edge.start() } else { edge.end() }, 0.0, None)
                    }
                    BoxItem::Atomic(index) => {
                        let laid = atomics.lay_out(self.atomics[*index], space, self.height);
                        (laid.margin_box.width, laid.margin_box.height, laid.baseline)
                    }
                    BoxItem::OutOfFlow(_) => (0.0, 0.0, None),
                    // Only its size: the line breaking places it.
                    BoxItem::Float(index) => {
                        let float = &mut self.floats[*index];
                        // The lines are broken again whenever it is placed:
                        // its size is in the bands it leaves otherwise.
                        float.margin_box = atomics.lay_out(float.id, space, self.height).margin_box;
                        (0.0, 0.0, None)
                    }
                };
                changed |= (inline_box.width, inline_box.height, inline_box.baseline)
                    != (width, height, baseline);
                inline_box.width = width;
                inline_box.height = height;
                inline_box.baseline = baseline;
            });
        }
        if changed {
            self.broken = None;
        }
    }

    /// Sizes each parley box's width for an intrinsic size in `space`:
    /// the atomic inlines are only measured, which keeps what they were
    /// laid out at (taffy keeps one layout, and more measurements). The
    /// boxes are sized again at the next `break_lines`.
    fn resize_for_widths(&mut self, space: AvailableSpace, atomics: &mut dyn AtomicInlines) {
        self.sized_for = None;
        self.broken = None;
        self.percent_base = 0.0;
        let boxes = self.layout.inline_boxes().len();
        for index in 0..boxes {
            self.layout.edit_inline_box(index, |inline_box| {
                inline_box.width = match &self.boxes[box_index(inline_box.id)] {
                    BoxItem::Edge { element, start } => {
                        let edge = Edge::of(&self.elements[*element].style, 0.0);
                        if *start { edge.start() } else { edge.end() }
                    }
                    BoxItem::Atomic(index) => atomics.width(self.atomics[*index], space),
                    BoxItem::OutOfFlow(_) | BoxItem::Float(_) => 0.0,
                };
            });
        }
    }

    /// The lines were laid out for real as they were broken last: their
    /// fragments are made from lines broken beside the same floats, its
    /// own floats among them where they were placed.
    pub(crate) fn keep_laid_out_beside(&mut self) {
        if let Some((_, beside, _)) = &self.broken {
            self.laid_out_beside = beside.clone();
        }
    }

    /// The floats beside the lines when they were last laid out for real.
    pub(crate) fn laid_out_beside(&self) -> &FloatBands {
        &self.laid_out_beside
    }

    /// Breaks lines at `width` (content-box px; `None` for no limit),
    /// beside the floats `bands` leave room around, and aligns them; its
    /// own floats are placed beside them if `atomics` places floats.
    /// Returns the size the lines take.
    pub(crate) fn break_lines(
        &mut self,
        width: Option<f32>,
        bands: &FloatBands,
        atomics: &mut dyn AtomicInlines,
    ) -> SizeF {
        // The block's content box is the boxes' containing block; with no
        // limit it has no definite width.
        self.resize_boxes(
            width.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
            atomics,
        );
        // Placing the floats puts them into the block formatting context,
        // which is another each time.
        let places = width.is_some() && !self.floats.is_empty() && atomics.places_floats();
        if !places
            && let Some((broken_at, beside, size)) = &self.broken
            && *broken_at == width
            && beside == bands
        {
            return *size;
        }
        atomics.count_line_break();
        // A width of exactly the content's own (a shrink-to-fit or
        // fit-content box) comes back with float noise from its border box
        // and padding: lines may run that far past it, as Blink's layout
        // units are 1/64 px. They are aligned in the width itself.
        self.layout
            .set_text_indent(indent_px(self.text_indent, width), IndentOptions::default());
        // The lines are aligned as the breaking ends.
        let alignment = match self.text_align {
            TextAlign::Start => Alignment::Start,
            TextAlign::End => Alignment::End,
            TextAlign::Left => Alignment::Left,
            TextAlign::Right => Alignment::Right,
            TextAlign::Center => Alignment::Center,
        };
        self.layout
            .set_alignment(alignment, AlignmentOptions::default());
        let mut beside = bands.clone();
        match width {
            Some(width) if places || !bands.is_empty() => {
                self.break_beside_floats(width, &mut beside, places, atomics);
            }
            _ => {
                let mut breaker = self.layout.break_lines();
                let max_advance = width.map_or(f32::MAX, |width| width + BREAK_TOLERANCE);
                breaker.state_mut().set_layout_max_advance(max_advance);
                breaker.state_mut().set_line_max_advance(max_advance);
                while let Some(yielded) = breaker.break_next() {
                    match yielded {
                        YieldData::LineBreak(_) => {
                            if let Some(width) = width {
                                breaker.set_prior_line_width(width);
                            }
                        }
                        // A float stays where it was placed.
                        YieldData::InlineBoxBreak(float) => {
                            breaker.state_mut().append_inline_box_to_line(
                                float.advance,
                                f32::NEG_INFINITY,
                                f32::NEG_INFINITY,
                            );
                        }
                        // No line height limit.
                        YieldData::MaxHeightExceeded(_) => {
                            unreachable!("parley yields no height limit here")
                        }
                    }
                }
                breaker.finish();
            }
        }
        // Lines beside floats skip down past them: the content ends at the
        // last line's bottom, not at their heights added up; a last line
        // with nothing on it (white space hanging after the last) doesn't
        // count, as parley's own height leaves it out.
        let height = if beside.is_empty() {
            self.layout.height()
        } else {
            // Any line but the last counts.
            let last = self.layout.len().saturating_sub(1);
            self.layout
                .lines()
                .enumerate()
                .rfind(|(i, line)| *i < last || line.items().next().is_some())
                .map_or(0.0, |(_, line)| line.metrics().block_max_coord)
        };
        let size = SizeF::new(self.layout.width(), height);
        self.broken = Some((width, beside, size));
        size
    }

    /// Breaks the lines one at a time beside the floats `bands` leave room
    /// around (Blink's `InlineLayoutAlgorithm` trying each layout
    /// opportunity): each in the room every band it reaches leaves, again
    /// in less room if it turns out taller than the band it started in,
    /// and lower down if its content doesn't fit where a band lower down
    /// has more room (white space hanging past its end doesn't count).
    ///
    /// If `places`, its floats are placed as they come (Blink's
    /// `LineBreaker::HandleFloat`): one that fits in what is left of its
    /// line, with no float before it on the line put below it, goes beside
    /// the line, and the line is broken again beside it; another goes below
    /// the line. Only once the line is final, as the block formatting
    /// context can't take a float back: one that turns out on the next
    /// line, the line broken before it, takes no room from this one (Blink's
    /// `RewindFloats`). `bands` then takes the room they leave too.
    fn break_beside_floats(
        &mut self,
        width: f32,
        bands: &mut FloatBands,
        places: bool,
        atomics: &mut dyn AtomicInlines,
    ) {
        // Without placing, every float is where it was placed last.
        let mut placed = vec![!places; self.floats.len()];
        let mut breaker = self.layout.break_lines();
        breaker
            .state_mut()
            .set_layout_max_advance(width + BREAK_TOLERANCE);
        let mut top = 0.0_f32;
        loop {
            let start = breaker.state().clone();
            // How tall the line is, once broken: 0 until then.
            let mut height = 0.0;
            // The floats that took room from the line and turned out not on
            // it: below it, if on it after all.
            let mut skipped: Vec<usize> = Vec::new();
            let done = loop {
                let (x, room) = bands.room(top, height, width);
                let state = breaker.state_mut();
                state.set_line_x(x);
                state.set_line_y(f64::from(top));
                state.set_line_max_advance(room + BREAK_TOLERANCE);
                // The floats met on the line and not placed yet, each with
                // the line's advance before it: those beside the line, with
                // the room they leave it, and those below it.
                let mut beside: Vec<(usize, f32)> = Vec::new();
                let mut below: Vec<(usize, f32)> = Vec::new();
                let mut left = room;
                let line = loop {
                    match breaker.break_next() {
                        None => break None,
                        Some(YieldData::LineBreak(line)) => break Some(line),
                        Some(YieldData::InlineBoxBreak(float)) => {
                            if let BoxItem::Float(index) =
                                self.boxes[box_index(float.inline_box_id)]
                                && !placed[index]
                            {
                                // Negative margins can make its margin box
                                // narrower than nothing: it leaves the line
                                // no wider than its room.
                                let size = self.floats[index].margin_box.width.max(0.0);
                                if below.is_empty()
                                    && !skipped.contains(&index)
                                    && float.advance + size <= left + BREAK_TOLERANCE
                                {
                                    left -= size;
                                    breaker
                                        .state_mut()
                                        .set_line_max_advance(left + BREAK_TOLERANCE);
                                    beside.push((index, float.advance));
                                } else {
                                    below.push((index, float.advance));
                                }
                            }
                            breaker.state_mut().append_inline_box_to_line(
                                float.advance,
                                f32::NEG_INFINITY,
                                f32::NEG_INFINITY,
                            );
                        }
                        Some(YieldData::MaxHeightExceeded(_)) => {
                            unreachable!("parley yields no height limit here")
                        }
                    }
                };
                let Some(line) = line else {
                    break true;
                };
                let on_line = |&&(_, at): &&(usize, f32)| at <= line.metrics.advance;
                let content = line.metrics.advance - line.metrics.hanging_advance;
                // Its content doesn't fit: a band lower down with more room.
                // Floats beside it are placed first, at its top, and it goes
                // on down past them too (Blink places a float that fits
                // where it comes, then tries the line in the next layout
                // opportunity).
                if content > left + BREAK_TOLERANCE
                    && beside.is_empty()
                    && let Some(lower) = bands.next_top(top)
                {
                    atomics.count_line_broken_again();
                    breaker.revert_to(start.clone());
                    top = lower;
                    height = 0.0;
                    continue;
                }
                // A line taller than its band again in less room: what it
                // reaches only narrows as it grows, so this settles.
                let tall = base::numerics::f32_from_f64(line.line_y_end - line.line_y_start);
                if tall > height && bands.room(top, tall, width).1 + BREAK_TOLERANCE < room {
                    atomics.count_line_broken_again();
                    breaker.revert_to(start.clone());
                    height = tall;
                    continue;
                }
                // Those that took room from it and turned out on the next
                // line: broken again without them.
                let rewound: Vec<usize> = beside
                    .iter()
                    .filter(|float| !on_line(float))
                    .map(|&(index, _)| index)
                    .collect();
                if !rewound.is_empty() {
                    skipped.extend(rewound);
                    atomics.count_line_broken_again();
                    breaker.revert_to(start.clone());
                    continue;
                }
                // Those beside it, at its top, and the line broken again
                // beside them where they went.
                if !beside.is_empty() {
                    for &(index, _) in &beside {
                        self.floats[index].placed = atomics.place_float(
                            self.floats[index].id,
                            self.floats[index].margin_box,
                            top,
                        );
                        placed[index] = true;
                    }
                    *bands = atomics.float_bands();
                    atomics.count_line_broken_again();
                    breaker.revert_to(start.clone());
                    continue;
                }
                breaker.set_prior_line_width(room);
                top = base::numerics::f32_from_f64(line.line_y_end);
                // Those below it, from its bottom.
                let mut any = false;
                for &(index, _) in below.iter().filter(on_line) {
                    self.floats[index].placed = atomics.place_float(
                        self.floats[index].id,
                        self.floats[index].margin_box,
                        top,
                    );
                    placed[index] = true;
                    any = true;
                }
                if any {
                    *bands = atomics.float_bands();
                }
                break false;
            };
            if done {
                break;
            }
        }
        breaker.finish();
    }

    /// The narrowest and widest the content can be laid out (css-sizing 3,
    /// "cyclic percentage"): percentages in the edges count as 0, and the
    /// atomic inlines take their min-content and max-content
    /// contributions. The floats' are the widest of them, and those beside
    /// each other side by side, beside the widest line (Blink's
    /// `InlineNode::ComputeMinMaxSizes`, which adds them to the line they
    /// are on).
    pub(crate) fn content_widths(&mut self, atomics: &mut dyn AtomicInlines) -> (f32, f32) {
        if let Some(widths) = self.content_widths {
            return widths;
        }
        // The indent counts, a percentage as 0 (Blink's `ComputeMinMaxSizes`
        // resolves it against an indefinite width).
        self.layout
            .set_text_indent(indent_px(self.text_indent, None), IndentOptions::default());
        self.resize_for_widths(AvailableSpace::MinContent, atomics);
        let mut min = self.layout.calculate_content_widths().min;
        self.resize_for_widths(AvailableSpace::MaxContent, atomics);
        let mut max = self.layout.calculate_content_widths().max;
        if !self.floats.is_empty() {
            let floats: Vec<taffy::NodeId> = self.floats.iter().map(|f| f.id).collect();
            min = min.max(atomics.float_widths(&floats, AvailableSpace::MinContent));
            max += atomics.float_widths(&floats, AvailableSpace::MaxContent);
        }
        let widths = (min, max);
        self.content_widths = Some(widths);
        widths
    }

    /// The first line's baseline as broken last, from the content box's
    /// top; `None` with no lines. An empty line (only out-of-flow boxes,
    /// floats and empty inline elements, at no height) has none to give:
    /// the next one's (Blink skips empty line boxes for the baseline).
    pub(crate) fn first_baseline(&self) -> Option<f32> {
        self.layout
            .lines()
            .find(|line| self.holds_content(line))
            .map(|line| line.metrics().baseline)
    }

    /// Where the first line as broken last starts (past the floats beside
    /// it, before its alignment's offset), and its baseline, from the
    /// content box's corner; `None` with no lines.
    pub(crate) fn first_line(&self) -> Option<(f32, f32)> {
        self.layout.lines().next().map(|line| {
            let metrics = line.metrics();
            (metrics.inline_min_coord, metrics.baseline)
        })
    }

    /// Whether its lines as broken last hold text or an atomic inline:
    /// not only out-of-flow boxes, floats and empty inline elements.
    pub(crate) fn has_content(&self) -> bool {
        self.layout.lines().any(|line| self.holds_content(&line))
    }

    /// Whether `line` holds text, a forced break (a `<br>` alone has no
    /// glyph) or an atomic inline: else it is empty (Blink's
    /// `LineInfo::IsEmptyLine`).
    fn holds_content(&self, line: &parley::Line<'_, u32>) -> bool {
        line.break_reason() == parley::BreakReason::Explicit
            || line.items().any(|item| match item {
                PositionedLayoutItem::GlyphRun(_) => true,
                PositionedLayoutItem::InlineBox(inline_box) => {
                    matches!(self.boxes[box_index(inline_box.id)], BoxItem::Atomic(_))
                }
            })
    }

    /// The last line's baseline as broken last, from the content box's
    /// top; `None` with no lines. An empty line has none to give: the one
    /// before's (as [`Self::first_baseline`]).
    pub(crate) fn last_baseline(&self) -> Option<f32> {
        self.layout
            .lines()
            .rev()
            .find(|line| self.holds_content(line))
            .map(|line| line.metrics().baseline)
    }

    /// The text nodes with text left here after white space processing,
    /// and the `<br>`s (a node may come more than once).
    pub(crate) fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.sources
            .iter()
            .flatten()
            .map(|source| source.node)
            .chain(self.breaks.iter().map(|&(.., br)| br))
    }

    /// The lines as broken last, offset by `origin` (the content box's
    /// position in the fragment), where each atomic inline's margin box is,
    /// in order, and the text's clusters and glyphs placed meanwhile.
    pub(crate) fn fragments(&self, origin: PointF) -> (Vec<LineFragment>, Vec<PointF>, TextPlaced) {
        let mut lines = Vec::new();
        let mut text = TextPlaced::default();
        let mut placed = vec![origin; self.atomics.len()];
        // Elements whose box goes on past the current line, outermost
        // first.
        let mut open: Vec<usize> = self.open_at_start.clone();
        // How far `position: relative` moves what is in the open elements
        // (css-position 3, "relative positioning"): theirs added up.
        let moved = |open: &[usize]| {
            open.iter().fold(PointF::new(0.0, 0.0), |sum, &element| {
                let by = relative_offset(
                    &self.elements[element].style,
                    self.percent_base,
                    self.height,
                );
                PointF::new(sum.x + by.x, sum.y + by.y)
            })
        };
        for line in self.layout.lines() {
            let metrics = line.metrics();
            let top = origin.y + metrics.block_min_coord;
            let baseline = origin.y + metrics.baseline;
            // Where the line's content ends: white space hanging past it
            // is removed (css-text phase II), so it gets no decorations
            // and is in no inline box.
            // A line broken after a soft hyphen ends with a hyphen, which
            // parley leaves room for at its end: on the left of its items
            // in a right-to-left paragraph.
            let hyphen_room = if self.layout.is_rtl() {
                metrics.hyphen_advance
            } else {
                0.0
            };
            let mut start = origin.x + metrics.inline_min_coord + metrics.offset - hyphen_room;
            let mut content_end = start + metrics.advance - metrics.hanging_advance;
            // The run the soft hyphen ending the line is in, and its
            // baseline; the hyphen goes past the line's text on its end
            // side (the paragraph's direction).
            let mut hyphen = None;
            let (mut text_left, mut text_right) = (f32::INFINITY, f32::NEG_INFINITY);
            let text_end = line.text_range().end;
            let mut runs: Vec<TextFragment> = Vec::new();
            let mut atomic_inlines: Vec<AtomicInline> = Vec::new();
            // The line's in-flow children in order, for `text-overflow`:
            // each run and atomic inline, with its left and right edges.
            let mut children: Vec<(LineChild, f32, f32)> = Vec::new();
            let mut boxes: Vec<LineBox> = open
                .iter()
                .enumerate()
                .map(|(i, &element)| LineBox {
                    moved: moved(&open[..=i]),
                    ..LineBox::continued(element)
                })
                .collect();
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        if metrics.hyphen_advance > 0.0 {
                            let left = origin.x + glyph_run.offset();
                            text_left = text_left.min(left);
                            text_right = text_right.max(left + glyph_run.advance());
                            if hyphen.is_none() && ends_with(&glyph_run, text_end) {
                                let brush = glyph_run.style().brush as usize;
                                let baseline = origin.y + glyph_run.baseline();
                                hyphen = Some((brush, baseline, moved(&open)));
                            }
                        }
                        let mut run = self.text_fragment(&glyph_run, origin, content_end);
                        for line_box in boxes.iter_mut().filter(|b| open.contains(&b.element)) {
                            let of = self.elements[line_box.element].node;
                            let own = run.node == Some(of.node) && run.pseudo == of.pseudo;
                            line_box.cover(&run, own);
                        }
                        // The boxes are moved as a whole, after this.
                        run.translate(moved(&open));
                        children.push((LineChild::Run(runs.len()), run.x, run.x + run.advance));
                        runs.push(run);
                    }
                    PositionedLayoutItem::InlineBox(inline_box) => {
                        let x = origin.x + inline_box.x;
                        let (element, start) = match &self.boxes[box_index(inline_box.id)] {
                            BoxItem::Edge { element, start } => (*element, *start),
                            // Not on the line: its containing block lays
                            // it out, or the line breaking placed it.
                            BoxItem::OutOfFlow(_) | BoxItem::Float(_) => continue,
                            BoxItem::Atomic(index) => {
                                let by = moved(&open);
                                placed[*index] =
                                    PointF::new(x + by.x, origin.y + inline_box.y + by.y);
                                // The elements around it widen to its margin box.
                                for line_box in
                                    boxes.iter_mut().filter(|b| open.contains(&b.element))
                                {
                                    line_box.left = line_box.left.min(x);
                                    line_box.right = line_box.right.max(x + inline_box.width);
                                }
                                children.push((
                                    LineChild::Atomic(atomic_inlines.len()),
                                    x + by.x,
                                    x + by.x + inline_box.width,
                                ));
                                atomic_inlines.push(AtomicInline {
                                    child: *index,
                                    paint_before: runs.len(),
                                    after_boxes: boxes.len(),
                                    hidden: false,
                                });
                                continue;
                            }
                        };
                        let edge = Edge::of(&self.elements[element].style, self.percent_base);
                        if start {
                            open.push(element);
                            let mut line_box = LineBox::started(element, runs.len());
                            line_box.moved = moved(&open);
                            line_box.left = x + edge.margin.left;
                            line_box.right = line_box.right.max(line_box.left);
                            boxes.push(line_box);
                        } else {
                            open.retain(|&e| e != element);
                            if let Some(line_box) = boxes.iter_mut().find(|b| b.element == element)
                            {
                                line_box.last = true;
                                line_box.right = x + inline_box.width - edge.margin.right;
                                line_box.left = line_box.left.min(line_box.right);
                            }
                        }
                    }
                }
            }
            if let Some((brush, baseline, by)) = hyphen
                && let Some(Some(fragment)) = self.hyphens.get(brush)
            {
                let x = if self.layout.is_rtl() {
                    text_left - fragment.advance
                } else {
                    text_right
                };
                let at = PointF::new(x, baseline);
                let mut fragment = fragment.clone();
                let (style, owner) = self.styles[brush].clone();
                fragment.node = owner.map(|owner| owner.node);
                fragment.pseudo = owner.and_then(|owner| owner.pseudo);
                fragment.style = style;
                fragment.translate(at);
                fragment.translate(by);
                children.push((
                    LineChild::Run(runs.len()),
                    fragment.x,
                    fragment.x + fragment.advance,
                ));
                runs.push(fragment);
            }
            let index = lines.len();
            // A line clamp's last line ends with the ellipsis whether it
            // overflows or not (Blink's `LineTruncator::TruncateLine` for
            // line clamp), placed before the line is aligned: an aligned
            // line moves over by what the ellipsis takes, as far as there is
            // room. The lines after it are hidden.
            let clamped = self.clamp == Some(index);
            if clamped && let Some(ellipsis) = &self.ellipsis {
                let free =
                    metrics.inline_max_coord - metrics.inline_min_coord - (content_end - start);
                if metrics.offset > 0.0 && free > 0.0 {
                    let shift = metrics.offset / free * free.min(ellipsis.advance);
                    let by = PointF::new(-shift, 0.0);
                    for run in &mut runs {
                        run.translate(by);
                    }
                    for (_, left, right) in &mut children {
                        *left -= shift;
                        *right -= shift;
                    }
                    for line_box in &mut boxes {
                        line_box.left -= shift;
                        line_box.right -= shift;
                    }
                    for atomic in &atomic_inlines {
                        placed[atomic.child].x -= shift;
                    }
                    start -= shift;
                    content_end -= shift;
                }
            }
            let boxes = boxes
                .into_iter()
                .map(|line_box| {
                    self.inline_box_fragment(line_box, baseline, (top, top + metrics.line_height))
                })
                .collect();
            // The line's end: the block's content box, or the floats beside
            // it.
            let line_end = origin.x + metrics.inline_max_coord;
            if let Some(ellipsis) = &self.ellipsis
                && (clamped
                    || (self.truncates
                        && matches!(self.broken.as_ref().map(|b| b.0), Some(Some(_)))
                        && content_end > line_end + BREAK_TOLERANCE))
            {
                truncate_line(
                    ellipsis,
                    line_end,
                    baseline,
                    &children,
                    &mut runs,
                    &mut atomic_inlines,
                    &mut text.glyphs,
                );
                // A blank clamped line has the ellipsis where it starts.
                if clamped && children.is_empty() {
                    let mut ellipsis = ellipsis.clone();
                    ellipsis.translate(PointF::new(start, baseline));
                    runs.push(ellipsis);
                }
            }
            #[expect(clippy::arithmetic_side_effects, reason = "index < self.layout.len()")]
            let last = index + 1 == self.layout.len();
            let end = self.line_end(&line, &runs, &children, origin, last);
            for run in runs.iter().filter(|run| run.clusters_made()) {
                text.clusters = text.clusters.saturating_add(run.clusters().len());
            }
            lines.push(LineFragment {
                rect: RectF::new(
                    start,
                    top,
                    metrics.advance - metrics.hanging_advance,
                    metrics.line_height,
                ),
                runs,
                boxes,
                atomic_inlines,
                hidden: self.clamp.is_some_and(|clamp| index > clamp),
                end,
            });
        }
        (lines, placed, text)
    }

    /// How `line`, with `runs` and `children`, ends: in a forced break if
    /// it does (a `<br>`'s, or a newline's of its text), else in a soft
    /// wrap, or the end of the content if `last`, its top at `origin`'s y.
    /// `None` without text on it or a `<br>`, or if an atomic inline is
    /// the last thing on it (Chrome 152 draws no box after one but a
    /// `<br>`'s).
    fn line_end(
        &self,
        line: &parley::Line<'_, u32>,
        runs: &[TextFragment],
        children: &[(LineChild, f32, f32)],
        origin: PointF,
        last: bool,
    ) -> Option<LineEnd> {
        let explicit = line.break_reason() == parley::BreakReason::Explicit;
        let text_end = line.text_range().end;
        let text = runs.iter().rposition(|run| run.source.is_some());
        // Where an atomic inline ends, if one is the last thing on the
        // line: white space hanging past it is nothing.
        let atomic_end = children
            .iter()
            .rev()
            .find(|(child, left, right)| matches!(child, LineChild::Atomic(_)) || right > left)
            .and_then(|&(ref child, _, right)| {
                matches!(child, LineChild::Atomic(_)).then_some(right)
            });
        let br = explicit
            .then(|| {
                self.breaks
                    .iter()
                    .find(|&&(at, ..)| at as usize + 1 == text_end)
            })
            .flatten();
        if let Some(&(at, _, br)) = br {
            // The newline's run has no glyphs, but its font: the `<br>`'s.
            let run = line
                .runs()
                .find(|run| run.text_range().contains(&(at as usize)))?;
            let newline = run
                .clusters()
                .find(|cluster| cluster.text_range().contains(&(at as usize)))?;
            let (style, _) = self.styles[newline.style().brush as usize].clone();
            let font = run.font();
            let font = FontData {
                bytes: Arc::new(font.data.clone()),
                index: font.index,
                id: font.data.id(),
            };
            let metrics = run.font_metrics();
            let x = atomic_end
                .or(text.map(|text| runs[text].x + runs[text].advance))
                .unwrap_or(origin.x + line.metrics().inline_min_coord + line.metrics().offset);
            return Some(LineEnd {
                width: space_advance(&font, run.font_size()),
                baseline: origin.y + line.metrics().baseline,
                ascent: metrics.ascent,
                descent: metrics.descent,
                style,
                node: None,
                at: LineEndAt::Break { br, x },
            });
        }
        if atomic_end.is_some() && !explicit {
            return None;
        }
        let fragment = &runs[text?];
        let (text, source) = fragment.source.as_ref()?;
        // Where in the text is worked out from its clusters when asked: a
        // run with a part of a text node's data has some.
        let text = TextEnd {
            node: *text,
            glyphs: fragment.glyphs.clone(),
            x: fragment.x,
            end: fragment.x + fragment.advance,
            start: source.start,
        };
        let at = if explicit {
            LineEndAt::Newline { text, atomic_end }
        } else if last {
            LineEndAt::End(text)
        } else {
            LineEndAt::Wrap(text)
        };
        Some(LineEnd {
            width: space_advance(&fragment.font, fragment.font_size),
            baseline: fragment.baseline,
            ascent: fragment.ascent,
            descent: fragment.descent,
            style: fragment.style.clone(),
            node: fragment.node,
            at,
        })
    }

    /// Where each out-of-flow box here would have been as the lines were
    /// broken last, offset by `origin` (the content box's position in the
    /// fragment), which is `width` wide: its static-position rectangle, that
    /// its self-alignment aligns it in (css-position 3, "static position
    /// rectangle"; Blink's `InlineLayoutAlgorithm::PlaceOutOfFlowObjects`).
    /// One that was inline-level is where it is on its line, from the
    /// line's top to its bottom; one that was block-level would have started
    /// a block there, so it spans the content box, at the start of the next
    /// line if something comes before it on its own.
    pub(crate) fn static_positions(&self, origin: PointF, width: f32) -> Vec<RectF> {
        let mut positions =
            vec![RectF::new(origin.x, origin.y, width, 0.0); self.out_of_flow.len()];
        for line in self.layout.lines() {
            let metrics = line.metrics();
            let top = origin.y + metrics.block_min_coord;
            // Whether something in flow comes before, on this line.
            let mut content = false;
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(_) => content = true,
                    PositionedLayoutItem::InlineBox(inline_box) => {
                        match self.boxes[box_index(inline_box.id)] {
                            BoxItem::Atomic(_) => content = true,
                            BoxItem::Edge { .. } | BoxItem::Float(_) => {}
                            BoxItem::OutOfFlow(index) => {
                                let inline_level = !self.out_of_flow[index].1.is_block_level();
                                positions[index] = if inline_level {
                                    RectF::new(
                                        origin.x + inline_box.x,
                                        top,
                                        0.0,
                                        metrics.line_height,
                                    )
                                } else if content {
                                    RectF::new(origin.x, top + metrics.line_height, width, 0.0)
                                } else {
                                    RectF::new(origin.x, top, width, 0.0)
                                };
                            }
                        }
                    }
                }
            }
        }
        positions
    }

    fn text_fragment(
        &self,
        glyph_run: &parley::GlyphRun<'_, u32>,
        origin: PointF,
        content_end: f32,
    ) -> TextFragment {
        let run = glyph_run.run();
        let font = run.font();
        let run_metrics = run.font_metrics();
        let brush = glyph_run.style().brush as usize;
        let (style, owner) = self.styles[brush].clone();
        let synthesis = run.synthesis();
        let embolden = crate::font_features::embolden(
            &style.font,
            synthesis.embolden() || needs_synthetic_bold(style.font.weight, font),
        );
        let x = origin.x + glyph_run.offset();
        // A span of the shaped text the lines share: its glyphs are placed
        // and its clusters made when asked (Blink's `ShapeResultView`).
        let span = glyph_run.span();
        let shaped = self.layout.shaped_text();
        let text = self.sources[brush].clone();
        let source = text
            .as_ref()
            .and_then(|text| text.run_source(span.text_range(shaped)));
        let advance = glyph_run.advance().min(content_end - x).max(0.0);
        TextFragment {
            node: owner.map(|owner| owner.node),
            pseudo: owner.and_then(|owner| owner.pseudo),
            source,
            glyphs: TextGlyphs::Shaped(Arc::new(ShapedSpan {
                shaped: Arc::clone(shaped),
                span,
                source: text,
                clusters: OnceLock::new(),
            })),
            font: FontData {
                bytes: Arc::new(font.data.clone()),
                index: font.index,
                id: font.data.id(),
            },
            font_size: run.font_size(),
            embolden,
            skew: crate::font_features::skew(&style.font, synthesis.skew()),
            variations: crate::font_features::run_variations(run, Some(&style.font)),
            x,
            advance,
            hidden_from: None,
            visible_advance: advance,
            baseline: origin.y + glyph_run.baseline(),
            ascent: run_metrics.ascent,
            descent: run_metrics.descent,
            underline_offset: run_metrics.underline_offset,
            underline_size: run_metrics.underline_size,
            strikethrough_offset: run_metrics.strikethrough_offset,
            strikethrough_size: run_metrics.strikethrough_size,
            style,
        }
    }

    /// An element's border box on one line: its horizontal extent, and
    /// vertically its font's content area (ascent and descent around the
    /// baseline) plus padding and border, which don't take room in the
    /// line (CSS 2 § 10.6.1). The font's metrics come from the element's
    /// own text on the line, else from any text inside it, else the box
    /// fills the line.
    fn inline_box_fragment(
        &self,
        line_box: LineBox,
        baseline: f32,
        (line_top, line_bottom): (f32, f32),
    ) -> InlineBoxFragment {
        let element = &self.elements[line_box.element];
        let style = &element.style;
        let edge = Edge::of(style, self.percent_base);
        // Around its text's baseline (`vertical-align`), or the line's.
        let baseline = line_box.baseline.unwrap_or(baseline);
        let (ascent, descent) = line_box
            .metrics
            .unwrap_or((baseline - line_top, line_bottom - baseline));
        let border = Sides {
            top: style.border_width.top,
            bottom: style.border_width.bottom,
            left: if line_box.first {
                style.border_width.left
            } else {
                0.0
            },
            right: if line_box.last {
                style.border_width.right
            } else {
                0.0
            },
        };
        let top = baseline - ascent - edge.padding.top - border.top;
        let bottom = baseline + descent + edge.padding.bottom + border.bottom;
        let (left, right) = if line_box.left <= line_box.right {
            (line_box.left, line_box.right)
        } else {
            // Nothing of it on this line but an edge.
            (line_box.left, line_box.left)
        };
        let at = line_box.moved;
        let padding = Sides {
            left: if line_box.first {
                edge.padding.left
            } else {
                0.0
            },
            right: if line_box.last {
                edge.padding.right
            } else {
                0.0
            },
            ..edge.padding
        };
        InlineBoxFragment {
            node: element.node.node,
            pseudo: element.node.pseudo,
            style: style.clone(),
            rect: RectF::new(left + at.x, top + at.y, right - left, bottom - top),
            border,
            padding,
            first: line_box.first,
            last: line_box.last,
            paint_before: line_box.paint_before,
        }
    }
}

/// How far `position: relative` moves a box in a containing block
/// `width` wide and `height` high (if definite): by `left`, else by minus
/// `right`, and by `top`, else by minus `bottom`; a percentage of an
/// indefinite height is `auto` (css-position 3, "relative positioning").
pub(crate) fn relative_offset(style: &ComputedStyle, width: f32, height: Option<f32>) -> PointF {
    if style.position != Position::Relative {
        return PointF::new(0.0, 0.0);
    }
    let resolve = |v: LengthPercentageOrAuto, base: Option<f32>| match v {
        LengthPercentageOrAuto::Auto => None,
        LengthPercentageOrAuto::LengthPercentage(v) => match v.px() {
            Some(px) => Some(px),
            // A percentage, or a `calc()` with one, needs the base.
            None => base.map(|base| v.resolve(base)),
        },
    };
    let inset = &style.inset;
    let x = resolve(inset.left, Some(width))
        .or_else(|| resolve(inset.right, Some(width)).map(|right| -right))
        .unwrap_or(0.0);
    let y = resolve(inset.top, height)
        .or_else(|| resolve(inset.bottom, height).map(|bottom| -bottom))
        .unwrap_or(0.0);
    PointF::new(x, y)
}

/// One inline element's edges, resolved: margin, border and padding on
/// each side. Percentages are of the containing block's width, vertical
/// ones included (CSS 2 § 8.3, § 8.4).
pub(crate) struct Edge {
    pub(crate) margin: Sides<f32>,
    pub(crate) padding: Sides<f32>,
    pub(crate) border: Sides<f32>,
}

impl Edge {
    pub(crate) fn of(style: &ComputedStyle, percent_base: f32) -> Self {
        let lp = |v: LengthPercentage| v.resolve(percent_base);
        // `auto` margins are 0 on inline boxes (CSS 2 § 10.3.1).
        let lpa = |v: LengthPercentageOrAuto| match v {
            LengthPercentageOrAuto::Auto => 0.0,
            LengthPercentageOrAuto::LengthPercentage(v) => lp(v),
        };
        Self {
            margin: Sides {
                top: lpa(style.margin.top),
                right: lpa(style.margin.right),
                bottom: lpa(style.margin.bottom),
                left: lpa(style.margin.left),
            },
            padding: Sides {
                top: lp(style.padding.top),
                right: lp(style.padding.right),
                bottom: lp(style.padding.bottom),
                left: lp(style.padding.left),
            },
            border: style.border_width,
        }
    }

    /// A content box of `size` grown by padding and border, and margin
    /// too if `with_margin`.
    pub(crate) fn outset(&self, size: SizeF, with_margin: bool) -> SizeF {
        let (m, p, b) = (&self.margin, &self.padding, &self.border);
        let (mx, my) = if with_margin {
            (m.left + m.right, m.top + m.bottom)
        } else {
            (0.0, 0.0)
        };
        SizeF::new(
            size.width + p.left + p.right + b.left + b.right + mx,
            size.height + p.top + p.bottom + b.top + b.bottom + my,
        )
    }

    /// The start edge's width (left: horizontal LTR only).
    fn start(&self) -> f32 {
        self.margin.left + self.border.left + self.padding.left
    }

    fn end(&self) -> f32 {
        self.padding.right + self.border.right + self.margin.right
    }
}

/// An element's box on the line being built.
struct LineBox {
    element: usize,
    /// Whether the element starts / ends on this line.
    first: bool,
    last: bool,
    /// The border box's horizontal extent so far.
    left: f32,
    right: f32,
    /// The font's ascent and descent: from the element's own text if any,
    /// else from text inside it.
    metrics: Option<(f32, f32)>,
    own_metrics: bool,
    /// The baseline of that text (`vertical-align` moves it off the
    /// line's); `None` with no text on this line.
    baseline: Option<f32>,
    /// The run it paints before.
    paint_before: usize,
    /// How far `position: relative` moves it: its own, and that of the
    /// elements it is in.
    moved: PointF,
}

impl LineBox {
    /// For an element that starts on this line, before run `paint_before`.
    fn started(element: usize, paint_before: usize) -> Self {
        Self {
            first: true,
            paint_before,
            ..Self::continued(element)
        }
    }

    /// For an element carried over from the line before: it paints first.
    fn continued(element: usize) -> Self {
        Self {
            element,
            first: false,
            last: false,
            left: f32::INFINITY,
            right: f32::NEG_INFINITY,
            metrics: None,
            own_metrics: false,
            baseline: None,
            paint_before: 0,
            moved: PointF::new(0.0, 0.0),
        }
    }

    /// A run inside the element (`own`: directly, not in a descendant):
    /// widen the box to it, and take its font's metrics, preferring the
    /// element's own first run.
    fn cover(&mut self, run: &TextFragment, own: bool) {
        self.left = self.left.min(run.x);
        self.right = self.right.max(run.x + run.advance);
        if own && !self.own_metrics {
            self.metrics = Some((run.ascent, run.descent));
            self.baseline = Some(run.baseline);
            self.own_metrics = true;
        } else if self.metrics.is_none() {
            self.metrics = Some((run.ascent, run.descent));
            self.baseline = Some(run.baseline);
        }
    }
}

/// A byte range of the processed text, with the style and element of its
/// text, and where a text node's run is in its data.
type RunItem<'a> = (
    std::ops::Range<usize>,
    &'a Arc<ComputedStyle>,
    Option<Owner>,
    Option<TextSource>,
);

/// The runs of the processed text, in order, covering all of it.
type Runs<'a> = Vec<RunItem<'a>>;

/// Pushes the runs that start before byte `end`, each in a span of its own
/// carrying its brush (and no shift of its own: its element's span has
/// that), and keeps their styles and sources. A run whose style isn't
/// `enclosing`'s, the innermost open element's (or the container's), sets
/// its text properties too: its parent is an element with no span, a
/// `display: contents` one (Blink's `LayoutText` has its own style).
#[allow(clippy::too_many_arguments)]
fn push_runs<'a>(
    builder: &mut TreeBuilder<'_, u32>,
    text: &str,
    runs: &mut VecDeque<(usize, RunItem<'a>)>,
    end: usize,
    enclosing: &ComputedStyle,
    block: BlockText<'_>,
    styles: &mut Vec<(Arc<ComputedStyle>, Option<Owner>)>,
    sources: &mut Vec<Option<Arc<TextSource>>>,
    names: &mut usize,
) {
    while runs
        .front()
        .is_some_and(|(_, (range, ..))| range.start < end)
    {
        let (index, (range, style, node, source)) = runs.pop_front().expect("peeked");
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a style per run of the text: fewer than 2^32"
        )]
        let brush = index as u32;
        if std::ptr::eq(&**style, enclosing) {
            builder.push_style_modification_span(&[
                StyleProperty::Brush(brush),
                StyleProperty::VerticalAlign(VerticalAlign::BASELINE),
            ]);
        } else {
            let mut properties = text_properties(style, brush, block);
            *names = names.wrapping_add(property_family_names(&properties));
            properties.push(StyleProperty::VerticalAlign(VerticalAlign::BASELINE));
            builder.push_style_modification_span(&properties);
        }
        builder.push_text(&text[range]);
        builder.pop_style_span();
        styles.push((style.clone(), node));
        sources.push(source.map(Arc::new));
    }
}

/// Where a text node's run is in its data (Blink's `OffsetMapping`, for one
/// node): each character the run kept, by its byte offset in the processed
/// text, and its UTF-16 offset in the data, in order; white space that
/// collapsed away has none. And the data's length in UTF-16.
#[derive(PartialEq)]
pub(crate) struct TextSource {
    node: NodeId,
    /// Each kept character's byte offset, and where it starts and ends in
    /// the data (UTF-16).
    chars: Vec<(u32, u32, u32)>,
    length: u32,
}

/// A run's text node and the part of its data the run stands for
/// (`TextFragment::source`).
type RunSource = Option<(NodeId, std::ops::Range<u32>)>;

impl TextSource {
    /// A glyph run's part of the data, in UTF-16, from where its clusters'
    /// text is (`GlyphSpan::text_range`: where the first starts and the
    /// furthest ends, in bytes; `None` without clusters). A node's runs
    /// cover all of its data: what collapsed away before a character goes
    /// with the run before it, before the first with the first, after the
    /// last with the last.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "byte offsets into the text: past u32 only past 4 GB of it"
    )]
    fn run_source(&self, text: Option<(usize, usize)>) -> RunSource {
        let (first, end_byte) = text?;
        let first = self
            .char_at(first as u32)
            .map_or(self.length, |&(_, start, _)| start);
        let start = if self
            .chars
            .first()
            .is_some_and(|&(_, start, _)| start == first)
        {
            0
        } else {
            first
        };
        // The next character kept after the run, else the data's end.
        let end = self
            .char_at(end_byte as u32)
            .map_or(self.length, |&(_, start, _)| start);
        Some((self.node, start..end))
    }

    /// The clusters of `span` of `shaped`, in its data (UTF-16), placed
    /// from the run's start (`TextFragment::clusters`).
    pub(crate) fn clusters(
        &self,
        span: &parley::GlyphSpan,
        shaped: &parley::ShapedText,
    ) -> Box<[TextCluster]> {
        let mut at = 0.0;
        // Where the last lookup ended: the clusters come in visual order, so
        // in left-to-right text each one starts where the one before ended.
        let mut hint = 0;
        span.clusters(shaped)
            .map(|cluster| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "byte offsets into the text: past u32 only past 4 GB of it"
                )]
                let (first, last) = (
                    cluster.text_range.start as u32,
                    cluster.text_range.end as u32,
                );
                let start = self.index_from(first, &mut hint);
                let end = self.index_from(last, &mut hint);
                let placed = TextCluster {
                    start: self
                        .chars
                        .get(start)
                        .map_or(self.length, |&(_, start, _)| start),
                    end: end
                        .checked_sub(1)
                        .and_then(|end| self.chars.get(end))
                        .map_or(self.length, |&(_, _, end)| end),
                    x: at,
                };
                at += cluster.advance;
                placed
            })
            .collect()
    }

    /// Where `clusters`' first would start, working out that one alone.
    pub(crate) fn first_cluster_start(
        &self,
        span: &parley::GlyphSpan,
        shaped: &parley::ShapedText,
    ) -> Option<u32> {
        let cluster = span.clusters(shaped).next()?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "byte offsets into the text: past u32 only past 4 GB of it"
        )]
        let first = cluster.text_range.start as u32;
        let start = self.index_from(first, &mut 0);
        Some(
            self.chars
                .get(start)
                .map_or(self.length, |&(_, start, _)| start),
        )
    }

    /// The kept character at `byte` or after it.
    fn char_at(&self, byte: u32) -> Option<&(u32, u32, u32)> {
        let index = self.chars.partition_point(|&(b, ..)| b < byte);
        self.chars.get(index)
    }

    /// How many kept characters are before `byte` (`partition_point`),
    /// looked for from `hint`, the last lookup's, which it then becomes:
    /// a few steps on when `byte` is at or past the character before it,
    /// a binary search otherwise (right-to-left text) or once those few
    /// steps don't reach it. A run's two binary searches a cluster, over
    /// all of a long text node's characters, were most of the time its
    /// fragments took.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "index - 1 only when index > 0, index + 1 only while index < chars.len(), \
                  and steps up to STEPS"
    )]
    fn index_from(&self, byte: u32, hint: &mut usize) -> usize {
        /// Steps taken one by one before the rest is searched.
        const STEPS: usize = 8;
        let chars = &self.chars;
        let mut index = *hint;
        if index > chars.len() || (index > 0 && chars[index - 1].0 >= byte) {
            index = chars.partition_point(|&(b, ..)| b < byte);
        } else {
            let mut steps = 0;
            while index < chars.len() && chars[index].0 < byte {
                index += 1;
                steps += 1;
                if steps == STEPS {
                    index += chars[index..].partition_point(|&(b, ..)| b < byte);
                    break;
                }
            }
        }
        debug_assert_eq!(
            index,
            chars.partition_point(|&(b, ..)| b < byte),
            "the kept characters before a byte, looked for from the last"
        );
        *hint = index;
        index
    }
}

/// Where each open and close tag sits in the processed text: a byte offset,
/// and the tag's index in the items.
type Tags = Vec<(usize, usize)>;

/// css-text's white space processing, phase I, across element boundaries
/// (like Blink's `InlineItemsBuilder`): the text for parley, where `\n` is
/// a forced break, the byte range each item's style covers, and where each
/// tag is. `None` if nothing is left.
///
/// Collapsible spaces fold into one space and are removed at the start of
/// a line and at the end of the content; spaces right before a forced
/// break go too. Spaces left at the end of a wrapped line hang, which
/// parley does. Tags are transparent to all of this: `a <b> b` keeps the
/// first space, before the tag, and a removed space takes no tag with it.
fn collapse_white_space(items: &[InlineItem]) -> Option<(String, Runs<'_>, Tags)> {
    // Each output character with the item it came from, so removing one
    // never leaves a run pointing at the wrong text, and where it starts
    // and ends in the item's text (UTF-16).
    let mut chars: Vec<(char, usize, u32, u32)> = Vec::new();
    // Each tag's position, in characters, with its item.
    let mut tags: Vec<(usize, usize)> = Vec::new();
    // Whether the last character is a space that collapsing produced.
    let mut last_collapsible = false;
    // At the start of a line, collapsible spaces are removed.
    let mut line_start = true;
    // Removes the last character; tags after it move back with it.
    let pop = |chars: &mut Vec<(char, usize, u32, u32)>, tags: &mut Vec<(usize, usize)>| {
        chars.pop();
        for tag in tags.iter_mut() {
            tag.0 = tag.0.min(chars.len());
        }
    };
    // The character before the current text, for `capitalize` (Blink's
    // `Capitalize` takes it too): a word going on from the previous text
    // node isn't a new one. A break or an atomic inline starts one.
    let mut previous: Option<char> = None;
    for (index, item) in items.iter().enumerate() {
        match item {
            InlineItem::Text(data, style, ..) => {
                let ws = style.white_space_collapse;
                let transform = style.text_transform;
                let word_starts =
                    (transform == TextTransform::Capitalize).then(|| word_starts(data, previous));
                // `-webkit-text-security` draws every character as its
                // mask, spaces too (Blink's `LayoutText::SecureText`).
                let mask = style.rare_inherited.text_security.mask();
                let mut utf16 = 0;
                for (byte, c) in data.char_indices() {
                    let at = utf16;
                    #[expect(
                        clippy::arithmetic_side_effects,
                        clippy::cast_possible_truncation,
                        reason = "a character is one or two UTF-16 units, and a text node's \
                                  data past u32 of them is past 8 GB"
                    )]
                    {
                        utf16 += c.len_utf16() as u32;
                    }
                    if let Some(mask) = mask {
                        chars.push((mask, index, at, utf16));
                        last_collapsible = false;
                        line_start = false;
                        continue;
                    }
                    if c == '\r' {
                        continue;
                    }
                    if c == '\n' && ws.preserves_newlines() {
                        if last_collapsible {
                            pop(&mut chars, &mut tags);
                        }
                        chars.push(('\n', index, at, utf16));
                        last_collapsible = false;
                        line_start = true;
                    } else if is_space(c) && ws.collapses_spaces() {
                        if !last_collapsible && !line_start {
                            chars.push((' ', index, at, utf16));
                            last_collapsible = true;
                        }
                    } else {
                        // A preserved space or tab, or any other character,
                        // as `text-transform` writes it: every character a
                        // case mapping expands it to maps to it.
                        let c = if c == '\n' { ' ' } else { c };
                        let starts_word = word_starts
                            .as_ref()
                            .is_some_and(|starts| starts.binary_search(&byte).is_ok());
                        for c in transformed(c, transform, starts_word) {
                            chars.push((c, index, at, utf16));
                        }
                        last_collapsible = false;
                        line_start = false;
                    }
                }
                if let Some(last) = data.chars().next_back() {
                    previous = Some(last);
                }
            }
            InlineItem::Break(..) => {
                if last_collapsible {
                    pop(&mut chars, &mut tags);
                }
                chars.push(('\n', index, 0, 0));
                last_collapsible = false;
                line_start = true;
                previous = None;
            }
            // Spaces collapse across an out-of-flow box (Blink's
            // `InlineItemsBuilder` leaves it out of white space processing).
            InlineItem::Open(..)
            | InlineItem::Close(..)
            | InlineItem::OutOfFlow(..)
            | InlineItem::Float(_) => tags.push((chars.len(), index)),
            // Content: spaces around it don't collapse into each other.
            // parley lays an in-flow box out as the object replacement
            // character at its place in the text, which its builder adds
            // (Blink's `InlineItemsBuilder` appends one too): it is here
            // so that every offset is one into the text parley has.
            InlineItem::Atomic(..) => {
                tags.push((chars.len(), index));
                chars.push((OBJECT_REPLACEMENT, index, 0, 0));
                last_collapsible = false;
                line_start = false;
                previous = None;
            }
        }
    }
    if last_collapsible {
        pop(&mut chars, &mut tags);
    }
    // Nothing to lay out: no text, no atomic inline (one alone still
    // makes a line), and no out-of-flow box or float to place.
    if chars.is_empty()
        && !tags.iter().any(|&(_, item)| {
            matches!(
                items[item],
                InlineItem::Atomic(..) | InlineItem::OutOfFlow(..) | InlineItem::Float(_)
            )
        })
    {
        return None;
    }

    let mut text = String::new();
    let mut runs: Runs<'_> = Vec::new();
    // The byte offset of each character, and of the end.
    // Only room asked for: saturating changes nothing.
    let mut offsets = Vec::with_capacity(chars.len().saturating_add(1));
    let mut last_item = usize::MAX;
    for (c, item, from, to) in chars {
        let start = text.len();
        offsets.push(start);
        text.push(c);
        // An atomic inline's character is in no run: parley's builder
        // adds it with the box.
        if matches!(items[item], InlineItem::Atomic(..)) {
            last_item = item;
            continue;
        }
        match runs.last_mut() {
            Some((range, ..)) if item == last_item => range.end = text.len(),
            // A new run: its item's text node, and the length of its data
            // (counted once a run, not once a character).
            _ => {
                let (style, node, source) = match &items[item] {
                    InlineItem::Text(data, style, node, text_node) => {
                        let source = text_node.map(|node| TextSource {
                            node,
                            chars: Vec::new(),
                            length: base::numerics::saturated_cast(data.encode_utf16().count()),
                        });
                        (style, *node, source)
                    }
                    InlineItem::Break(style, _) => (style, None, None),
                    InlineItem::Open(..)
                    | InlineItem::Close(..)
                    | InlineItem::Atomic(..)
                    | InlineItem::OutOfFlow(..)
                    | InlineItem::Float(_) => {
                        unreachable!("tags and boxes have no run")
                    }
                };
                runs.push((start..text.len(), style, node, source));
            }
        }
        if let Some((.., Some(source))) = runs.last_mut() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a byte offset into the text: past u32 only past 4 GB of it"
            )]
            let start = start as u32;
            source.chars.push((start, from, to));
        }
        last_item = item;
    }
    offsets.push(text.len());
    let tags = tags
        .into_iter()
        .map(|(char_index, item)| (offsets[char_index], item))
        .collect();
    Some((text, runs, tags))
}

/// `style` as a parley text style, in a block with `block`; `brush`
/// identifies the run. The font list ends in the UA's standard font.
fn text_style(
    style: &ComputedStyle,
    brush: u32,
    block: BlockText<'_>,
) -> TextStyle<'static, 'static, u32> {
    let families = crate::font_families::font_stack(
        &style.font.family,
        crate::font_families::Platform::CURRENT,
    );
    let features = crate::font_features::features(&style.font);
    let variations = crate::font_features::variations(&style.font);
    TextStyle {
        font_family: parley::FontFamily::List(Cow::Owned(families)),
        font_size: style.font.size,
        font_width: parley::FontWidth::from_ratio(style.font.stretch / 100.0),
        font_features: if features.is_empty() {
            parley::FontFeatures::empty()
        } else {
            parley::FontFeatures::List(Cow::Owned(features))
        },
        font_variations: if variations.is_empty() {
            parley::FontVariations::empty()
        } else {
            parley::FontVariations::List(Cow::Owned(variations))
        },
        font_weight: parley::FontWeight::new(style.font.weight),
        font_style: match style.font.style {
            FontStyle::Normal => parley::FontStyle::Normal,
            FontStyle::Italic => parley::FontStyle::Italic,
            FontStyle::Oblique => parley::FontStyle::Oblique(None),
        },
        line_height: match style.line_height {
            LineHeight::Normal => parley::LineHeight::MetricsRelative(1.0),
            // What parley would make of `FontSizeRelative`, at this font
            // size, kept in range.
            LineHeight::Number(n) => {
                parley::LineHeight::Absolute(LineHeight::number_px(n, style.font.size))
            }
            LineHeight::Px(px) => parley::LineHeight::Absolute(px),
        },
        text_wrap_mode: match style.text_wrap_mode {
            computed_style::TextWrapMode::Wrap => TextWrapMode::Wrap,
            computed_style::TextWrapMode::Nowrap => TextWrapMode::NoWrap,
        },
        // The white space was processed already (`collapse_white_space`):
        // parley keeps what it gets, but `break-spaces` makes preserved
        // spaces wrap instead of hanging at the end of a line.
        white_space_collapse: match style.white_space_collapse {
            WhiteSpaceCollapse::BreakSpaces => parley::WhiteSpaceCollapse::BreakSpaces,
            _ => parley::WhiteSpaceCollapse::Preserve,
        },
        word_break: match style.rare_inherited.word_break {
            WordBreak::Normal | WordBreak::BreakWord => parley::WordBreak::Normal,
            WordBreak::BreakAll => parley::WordBreak::BreakAll,
            WordBreak::KeepAll => parley::WordBreak::KeepAll,
        },
        // Chrome's `word-break: break-word` is `overflow-wrap: anywhere`.
        overflow_wrap: match (
            style.rare_inherited.overflow_wrap,
            style.rare_inherited.word_break,
        ) {
            (OverflowWrap::Normal, WordBreak::BreakWord) => parley::OverflowWrap::Anywhere,
            (OverflowWrap::Normal, _) => parley::OverflowWrap::Normal,
            (OverflowWrap::BreakWord, _) => parley::OverflowWrap::BreakWord,
            (OverflowWrap::Anywhere, _) => parley::OverflowWrap::Anywhere,
        },
        letter_spacing: style.rare_inherited.letter_spacing.unwrap_or(0.0),
        word_spacing: style.rare_inherited.word_spacing,
        tab_stops: block
            .tabs
            .map(|tabs| tabs.stops(style.rare_inherited.tab_size)),
        soft_hyphens: block.soft_hyphens(style),
        brush,
        ..TextStyle::default()
    }
}

/// What a run's parley style takes from the block it is in, for breaking
/// its lines.
#[derive(Clone, Copy, Default)]
struct BlockText<'a> {
    /// The tab stops, when there are tabs to place.
    tabs: Option<BlockTabs>,
    /// The advance of the hyphen a soft hyphen ends a line with, by the
    /// style of each run with one (`InlineContent::hyphens`).
    hyphens: &'a [(*const ComputedStyle, f32)],
}

impl BlockText<'_> {
    /// How a line may break after a soft hyphen in `style` (css-text 3
    /// § 5.4): not at all with `hyphens: none`, with a hyphen otherwise. A
    /// style no run with a soft hyphen has keeps parley's default: it
    /// breaks no line after one.
    fn soft_hyphens(self, style: &ComputedStyle) -> parley::SoftHyphens {
        if style.rare_inherited.hyphens == Hyphens::None {
            return parley::SoftHyphens::NoBreak;
        }
        self.hyphens
            .iter()
            .find(|&&(with, _)| std::ptr::eq(with, style))
            .map_or(parley::SoftHyphens::Break, |&(_, advance)| {
                parley::SoftHyphens::Hyphen(advance)
            })
    }
}

/// What tab stops are counted in: the block container's space (CSS Text 3,
/// `tab-size`, as Chrome has it): the space's advance in its font with its
/// letter and word spacing, and half the space, the least a tab advances
/// (Blink's `Font::TabWidth`).
#[derive(Clone, Copy)]
struct BlockTabs {
    space: f32,
    min_advance: f32,
}

impl BlockTabs {
    fn new(cx: &mut TextContext, container: &ComputedStyle) -> Self {
        // The space shaped without the spacing, which parley adds to it.
        let mut unspaced = container.clone();
        unspaced.rare_inherited_mut().letter_spacing = None;
        unspaced.rare_inherited_mut().word_spacing = 0.0;
        let layout = shape_line(cx, " ", &[(0..1, &unspaced)]);
        let advance = layout
            .lines()
            .next()
            .and_then(|line| line.runs().next())
            .and_then(|run| run.clusters().next().map(|cluster| cluster.advance()))
            .unwrap_or(container.font.size / 4.0);
        Self {
            space: advance
                + container.rare_inherited.letter_spacing.unwrap_or(0.0)
                + container.rare_inherited.word_spacing,
            min_advance: advance / 2.0,
        }
    }

    /// The tab stops for text in a style with `tab_size`.
    fn stops(self, tab_size: computed_style::TabSize) -> parley::TabStops {
        parley::TabStops {
            interval: match tab_size {
                computed_style::TabSize::Spaces(spaces) => spaces * self.space,
                computed_style::TabSize::Px(px) => px,
            },
            min_advance: self.min_advance,
        }
    }
}

/// How many font family names `family` has: each is looked up by name.
fn family_names(family: &parley::FontFamily<'_>) -> usize {
    match family {
        parley::FontFamily::List(list) => list.len(),
        parley::FontFamily::Single(_) => 1,
        parley::FontFamily::Source(source) => source.split(',').count(),
    }
}

/// How many font family names `properties` set.
fn property_family_names(properties: &[StyleProperty<'_, u32>]) -> usize {
    properties
        .iter()
        .map(|property| match property {
            StyleProperty::FontFamily(family) => family_names(family),
            _ => 0,
        })
        .fold(0, usize::wrapping_add)
}

/// The parley properties for text in `style`, as `text_style` has them;
/// `brush` identifies the run.
fn text_properties(
    style: &ComputedStyle,
    brush: u32,
    block: BlockText<'_>,
) -> Vec<StyleProperty<'static, u32>> {
    // Every one of them: an element's span starts as a copy of its
    // parent's, so a value equal to the default (`letter-spacing: normal`
    // under a spaced parent) has to be set too.
    let s = text_style(style, brush, block);
    vec![
        StyleProperty::FontFamily(s.font_family),
        StyleProperty::FontSize(s.font_size),
        StyleProperty::FontWeight(s.font_weight),
        StyleProperty::FontStyle(s.font_style),
        StyleProperty::FontWidth(s.font_width),
        StyleProperty::FontFeatures(s.font_features),
        StyleProperty::FontVariations(s.font_variations),
        StyleProperty::LineHeight(s.line_height),
        StyleProperty::TextWrapMode(s.text_wrap_mode),
        StyleProperty::WhiteSpaceCollapse(s.white_space_collapse),
        StyleProperty::WordBreak(s.word_break),
        StyleProperty::OverflowWrap(s.overflow_wrap),
        StyleProperty::LetterSpacing(s.letter_spacing),
        StyleProperty::WordSpacing(s.word_spacing),
        StyleProperty::TabStops(s.tab_stops),
        StyleProperty::SoftHyphens(s.soft_hyphens),
        StyleProperty::Brush(s.brush),
    ]
}

/// `style`'s `vertical-align` as parley has it: a length raises the baseline
/// by that much, a percentage of the box's `line-height` (the font's, for
/// `normal`). `sub` and `super` are parley's, a fifth and a third of the
/// font size (Blink adds 1px); the keywords are relative to the span parley
/// takes as the parent.
fn vertical_align(cx: &mut TextContext, style: &ComputedStyle) -> VerticalAlign {
    use computed_style::VerticalAlign as Css;
    match style.vertical_align {
        Css::Baseline => VerticalAlign::BASELINE,
        Css::Sub => VerticalAlign::SUB,
        Css::Super => VerticalAlign::SUPER,
        Css::TextTop => VerticalAlign::TEXT_TOP,
        Css::TextBottom => VerticalAlign::TEXT_BOTTOM,
        Css::Middle => VerticalAlign::MIDDLE,
        Css::Top => VerticalAlign::TOP,
        Css::Bottom => VerticalAlign::BOTTOM,
        Css::Length(length) => {
            let mut percent = |p: f32| {
                let line_height = match style.line_height {
                    LineHeight::Px(px) => px,
                    LineHeight::Number(n) => LineHeight::number_px(n, style.font.size),
                    LineHeight::Normal => normal_line_height(cx, style),
                };
                line_height * p / 100.0
            };
            VerticalAlign::length(match length {
                LengthPercentage::Px(px) => px,
                LengthPercentage::Percent(p) => percent(p),
                LengthPercentage::Calc { px, percent: p, .. } => px + percent(p),
            })
        }
    }
}

/// What a text control is sized by, of `style`'s first font (Blink's
/// `SimpleFontData` and `LayoutTextControl::GetAvgCharWidth`).
pub(crate) struct ControlFontMetrics {
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    /// The average character's width: `x`'s advance (Blink's when Skia
    /// gives no average, as on Windows), at least rounded (Blink's
    /// `max(width, roundf(width))`); `0`'s for a font whose average is off.
    pub(crate) avg_char_width: f32,
    /// What a text field adds to `size` average characters: the widest
    /// character's room past an average one's (Blink's `max_char_width`,
    /// the font's `head` bounds, rounded), 0 for a font whose average is
    /// off.
    pub(crate) max_char_adjustment: f32,
}

/// A space's advance in `font` at `size` px, by its `hmtx` (Blink's
/// `SimpleFontData::SpaceWidth`); 0 if the font has no space.
fn space_advance(font: &FontData, size: f32) -> f32 {
    use read_fonts::{FontRef, TableProvider};

    let Ok(face) = FontRef::from_index((*font.bytes).as_ref(), font.index) else {
        return 0.0;
    };
    let advance = || {
        let glyph = face.cmap().ok()?.map_codepoint(' ')?;
        let advance = face.hmtx().ok()?.advance(glyph)?;
        let units = face.head().ok()?.units_per_em();
        Some(f32::from(advance) * size / f32::from(units))
    };
    advance().unwrap_or(0.0)
}

pub(crate) fn control_font_metrics(
    cx: &mut TextContext,
    style: &ComputedStyle,
) -> ControlFontMetrics {
    use read_fonts::{FontRef, TableProvider};

    let layout = shape_line(cx, "x0", &[(0..2, style)]);
    let mut advances = [0.0f32; 2];
    let mut metrics = None;
    let mut x_range = None;
    if let Some(run) = layout.lines().next().and_then(|line| line.runs().next()) {
        let font = run.font();
        x_range = FontRef::from_index(font.data.data(), font.index)
            .ok()
            .and_then(|face| face.head().ok())
            // Apart as floats: the two ends of 16 bits each may be further
            // apart than 16 bits hold.
            .map(|head| {
                (f32::from(head.x_max()) - f32::from(head.x_min())) / f32::from(head.units_per_em())
            });
        metrics = Some(*run.font_metrics());
        for (advance, cluster) in advances.iter_mut().zip(run.clusters()) {
            *advance = cluster.advance();
        }
    }
    let (ascent, descent) = metrics.map_or((style.font.size, 0.0), |m| (m.ascent, m.descent));
    let [x, zero] = advances;
    let avg = x.max(x.round());
    // Blink's `HasValidAvgCharWidth`: an average far wider than `0`.
    if zero > 0.0 && avg > zero * 1.7 {
        return ControlFontMetrics {
            ascent,
            descent,
            avg_char_width: zero,
            max_char_adjustment: 0.0,
        };
    }
    let max_char_width = x_range.map_or(0.0, |range| (range * style.font.size).round());
    ControlFontMetrics {
        ascent,
        descent,
        avg_char_width: avg,
        max_char_adjustment: if max_char_width > 0.0 {
            max_char_width - avg
        } else {
            0.0
        },
    }
}

/// `line-height: normal` for `style`: its font's ascent, descent and line
/// gap (Blink's `FontMetrics::LineSpacing`), from the font parley picks for
/// its text.
fn normal_line_height(cx: &mut TextContext, style: &ComputedStyle) -> f32 {
    let layout = shape_line(cx, "x", &[(0..1, style)]);
    layout
        .lines()
        .next()
        .and_then(|line| line.runs().next())
        .map(|run| {
            let metrics = run.font_metrics();
            metrics.ascent + metrics.descent + metrics.leading
        })
        .unwrap_or(style.font.size)
}

/// `text-indent` in px for lines broken at `width` (`None`: no definite
/// width, a percentage is 0).
fn indent_px(indent: LengthPercentage, width: Option<f32>) -> f32 {
    let percent = |percent: f32| width.map_or(0.0, |width| width * percent / 100.0);
    match indent {
        LengthPercentage::Px(px) => px,
        LengthPercentage::Percent(p) => percent(p),
        LengthPercentage::Calc { px, percent: p, .. } => px + percent(p),
    }
}

/// Where the words of `data` start, as byte offsets (UAX #29's word
/// boundaries, with `previous`, the character before `data`, in front so a
/// word going on from it isn't a start): `capitalize` uppercases the first
/// character of each, as Blink's `Capitalize` does.
fn word_starts(data: &str, previous: Option<char>) -> Vec<usize> {
    let (text, offset) = match previous {
        Some(previous) => (format!("{previous}{data}"), previous.len_utf8()),
        None => (data.to_owned(), 0),
    };
    let mut starts = Vec::new();
    let mut start: usize = 0;
    for end in WordSegmenter::new_dictionary(Default::default()).segment_str(&text) {
        if end == 0 {
            continue;
        }
        if let Some(start) = start.checked_sub(offset) {
            starts.push(start);
        }
        start = end;
    }
    starts
}

/// `c` as `text-transform` writes it: itself, or the characters its case
/// mapping gives (`ß` uppercases to `SS`).
fn transformed(c: char, transform: TextTransform, starts_word: bool) -> CaseChars {
    match transform {
        TextTransform::Uppercase => CaseChars::Upper(c.to_uppercase()),
        TextTransform::Capitalize if starts_word => CaseChars::Upper(c.to_uppercase()),
        TextTransform::Lowercase => CaseChars::Lower(c.to_lowercase()),
        TextTransform::None | TextTransform::Capitalize => CaseChars::Same(Some(c)),
    }
}

enum CaseChars {
    Same(Option<char>),
    Upper(std::char::ToUppercase),
    Lower(std::char::ToLowercase),
}

impl Iterator for CaseChars {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        match self {
            Self::Same(c) => c.take(),
            Self::Upper(chars) => chars.next(),
            Self::Lower(chars) => chars.next(),
        }
    }
}

/// The index into an inline content's boxes of parley's inline box `id`:
/// the ids are those indexes, made from `usize`s.
#[expect(
    clippy::cast_possible_truncation,
    reason = "made from a usize, the index it is"
)]
fn box_index(id: u64) -> usize {
    id as usize
}

#[cfg(test)]
mod tests {
    use computed_style::WhiteSpace::{self, *};
    use read_fonts::FontRef;
    use read_fonts::types::Tag;

    use super::*;

    fn style(white_space: WhiteSpace) -> Arc<ComputedStyle> {
        let (white_space_collapse, text_wrap_mode) = white_space.longhands();
        Arc::new(ComputedStyle {
            white_space_collapse,
            text_wrap_mode,
            ..ComputedStyle::initial()
        })
    }

    fn text(ws: WhiteSpace, data: &str) -> InlineItem {
        InlineItem::Text(data.into(), style(ws), None, None)
    }

    /// The processed text, and each run's text.
    fn collapse(items: &[InlineItem]) -> Option<(String, Vec<String>)> {
        let (text, runs, _) = collapse_white_space(items)?;
        let runs = runs
            .iter()
            .map(|(r, ..)| text[r.clone()].to_owned())
            .collect();
        Some((text, runs))
    }

    /// The processed text with each tag shown as `<n>` or `</n>`.
    fn with_tags(items: &[InlineItem]) -> String {
        let (text, _, tags) = collapse_white_space(items).unwrap();
        let mut out = String::new();
        let mut rest = 0;
        for (offset, item) in tags {
            out.push_str(&text[rest..offset]);
            rest = offset;
            out.push_str(&match &items[item] {
                InlineItem::Open(..) => format!("<{item}>"),
                InlineItem::Close(..) => format!("</{item}>"),
                _ => unreachable!(),
            });
        }
        out.push_str(&text[rest..]);
        out
    }

    fn open() -> InlineItem {
        InlineItem::Open(Owner::element(NodeId::from_raw(1)), style(Normal))
    }

    fn close() -> InlineItem {
        InlineItem::Close(Owner::element(NodeId::from_raw(1)), style(Normal))
    }

    fn transformed_text(transform: TextTransform, data: &str) -> InlineItem {
        let style = Arc::new(ComputedStyle {
            text_transform: transform,
            ..ComputedStyle::initial()
        });
        InlineItem::Text(data.into(), style, None, None)
    }

    /// `text-transform` as css-text 3 has it: `uppercase` and `lowercase`
    /// with the full case mappings (`ß` to `SS`), `capitalize` the first
    /// character of each word (after punctuation too), a word going on
    /// from the previous text node not starting again; a break starts
    /// one.
    #[test]
    fn text_is_transformed() {
        use TextTransform::*;
        let one = |transform, data: &str| collapse(&[transformed_text(transform, data)]).unwrap().0;
        assert_eq!(one(Uppercase, "straße"), "STRASSE");
        assert_eq!(one(Lowercase, "ÀB"), "àb");
        assert_eq!(
            one(Capitalize, "hello  world-wide (x) 3rd"),
            "Hello World-Wide (X) 3rd"
        );
        assert_eq!(
            collapse(&[
                transformed_text(Capitalize, "he"),
                transformed_text(Capitalize, "llo wor"),
                transformed_text(Capitalize, "ld"),
            ])
            .unwrap()
            .0,
            "Hello World"
        );
        assert_eq!(
            collapse(&[
                transformed_text(Capitalize, "a"),
                InlineItem::Break(style(Normal), NodeId::from_raw(9)),
                transformed_text(Capitalize, "b"),
            ])
            .unwrap()
            .0,
            "A\nB"
        );
    }

    /// A character a case mapping expands maps to its source: each
    /// character produced has the source character's offsets.
    #[test]
    fn an_expanded_character_keeps_its_source() {
        let style = Arc::new(ComputedStyle {
            text_transform: TextTransform::Uppercase,
            ..ComputedStyle::initial()
        });
        let items = [InlineItem::Text(
            "aß".into(),
            style,
            None,
            Some(NodeId::from_raw(1)),
        )];
        let (text, runs, _) = collapse_white_space(&items).unwrap();
        assert_eq!(text, "ASS");
        let source = runs[0].3.as_ref().unwrap();
        assert_eq!(source.chars, [(0, 0, 1), (1, 1, 2), (2, 1, 2)]);
        assert_eq!(source.length, 2);
    }

    #[test]
    fn tags_keep_their_place_through_collapsing() {
        // `a <b> b</b>`: the space before the tag stays, the one after goes.
        assert_eq!(
            with_tags(&[text(Normal, "a "), open(), text(Normal, " b"), close()]),
            "a <1>b</3>"
        );
        // A space right before a close tag stays inside the element.
        assert_eq!(
            with_tags(&[open(), text(Normal, "a "), close(), text(Normal, " b")]),
            "<0>a </2>b"
        );
        // Spaces removed at the end, or before a forced break, don't take
        // the tags after them along.
        assert_eq!(
            with_tags(&[text(Normal, "a "), close(), open()]),
            "a</1><2>"
        );
        let br = InlineItem::Break(style(Normal), NodeId::from_raw(9));
        assert_eq!(
            with_tags(&[text(Normal, "a "), close(), br, text(Normal, "b")]),
            "a</1>\nb"
        );
        // Tags alone are nothing to lay out.
        assert!(InlineItem::is_collapsible_space_only(&[open(), close()]));
    }

    #[test]
    fn collapsing_spaces() {
        assert_eq!(collapse(&[text(Normal, "  a \t\n  b  ")]).unwrap().0, "a b");
        // Across element boundaries: a space after a space goes, wherever
        // it is.
        let (all, runs) =
            collapse(&[text(Normal, "a "), text(Normal, " b"), text(Normal, " c ")]).unwrap();
        assert_eq!(all, "a b c");
        assert_eq!(runs, ["a ", "b", " c"]);
        // Nothing left.
        assert!(collapse(&[text(Normal, " \n "), text(Nowrap, "\t")]).is_none());
    }

    #[test]
    fn preserving_spaces_and_newlines() {
        assert_eq!(
            collapse(&[text(Pre, "  a  \n b ")]).unwrap().0,
            "  a  \n b "
        );
        assert_eq!(collapse(&[text(PreWrap, "a\r\nb")]).unwrap().0, "a\nb");
        // pre-line keeps newlines but not the spaces around them.
        assert_eq!(collapse(&[text(PreLine, " a  \n  b ")]).unwrap().0, "a\nb");
        // A collapsible space after a preserved one stays.
        assert_eq!(
            collapse(&[text(Pre, "a "), text(Normal, " b")]).unwrap().0,
            "a  b"
        );
    }

    #[test]
    fn forced_breaks() {
        let br = InlineItem::Break(style(Normal), NodeId::from_raw(9));
        let (all, runs) = collapse(&[text(Normal, "a "), br, text(Normal, " b")]).unwrap();
        // The space before the break and after it (line start) both go.
        assert_eq!(all, "a\nb");
        assert_eq!(runs, ["a", "\n", "b"]);
        assert!(!InlineItem::is_collapsible_space_only(&[
            InlineItem::Break(style(Normal), NodeId::from_raw(9))
        ]));
        assert!(InlineItem::is_collapsible_space_only(&[text(
            Normal, " \n "
        )]));
        assert!(!InlineItem::is_collapsible_space_only(&[text(Pre, " ")]));
    }

    /// Lato with some of its tables replaced or added. The directory stays
    /// sorted by tag, which table lookups rely on; checksums are left at 0,
    /// which nothing here reads.
    fn lato_with(tables: &[(&[u8; 4], Vec<u8>)]) -> parley::FontData {
        let lato = FontRef::new(LATO).unwrap();
        let mut all: Vec<(Tag, Vec<u8>)> = lato
            .table_directory()
            .table_records()
            .iter()
            .map(|record| record.tag())
            .filter(|tag| !tables.iter().any(|(t, _)| Tag::new(t) == *tag))
            .map(|tag| (tag, lato.table_data(tag).unwrap().as_bytes().to_vec()))
            .chain(
                tables
                    .iter()
                    .map(|(tag, data)| (Tag::new(tag), data.clone())),
            )
            .collect();
        all.sort_by_key(|(tag, _)| *tag);
        let mut font = Vec::new();
        font.extend(0x0001_0000u32.to_be_bytes());
        font.extend((all.len() as u16).to_be_bytes());
        // searchRange, entrySelector, rangeShift: unused by read-fonts.
        font.extend([0; 6]);
        let mut offset = 12 + 16 * all.len();
        for (tag, data) in &all {
            font.extend(tag.to_be_bytes());
            font.extend(0u32.to_be_bytes());
            font.extend((offset as u32).to_be_bytes());
            font.extend((data.len() as u32).to_be_bytes());
            offset += data.len().next_multiple_of(4);
        }
        for (_, data) in &all {
            font.extend(data);
            font.resize(font.len().next_multiple_of(4), 0);
        }
        parley::FontData::new(Blob::new(Arc::new(font)), 0)
    }

    #[test]
    fn synthetic_bold_needs_a_bold_weight_on_a_lighter_face() {
        let medium = parley::FontData::new(Blob::new(Arc::new(LATO)), 0);
        assert!(needs_synthetic_bold(700.0, &medium));
        assert!(needs_synthetic_bold(600.0, &medium));
        assert!(!needs_synthetic_bold(599.0, &medium));
        // The rebuilt font below still reads as Lato Medium.
        assert!(needs_synthetic_bold(700.0, &lato_with(&[])));

        // A semibold face (usWeightClass at offset 4 of OS/2) is bold enough.
        let mut os2 = FontRef::new(LATO)
            .unwrap()
            .table_data(Tag::new(b"OS/2"))
            .unwrap()
            .as_bytes()
            .to_vec();
        os2[4..6].copy_from_slice(&600u16.to_be_bytes());
        assert!(!needs_synthetic_bold(700.0, &lato_with(&[(b"OS/2", os2)])));

        // A `wght` axis gives the weight; fontique sets it.
        let fvar = [
            &[0, 1, 0, 0][..],          // version 1.0
            &[0, 16, 0, 2],             // axesArrayOffset, reserved
            &[0, 1, 0, 20, 0, 0, 0, 8], // 1 axis of 20 bytes, no instances
            b"wght",
            &(100u32 << 16).to_be_bytes(),
            &(500u32 << 16).to_be_bytes(),
            &(900u32 << 16).to_be_bytes(),
            &[0, 0, 1, 0], // flags, axisNameID 256
        ]
        .concat();
        assert!(!needs_synthetic_bold(700.0, &lato_with(&[(b"fvar", fvar)])));
    }
}
