//! Layout (`blink/renderer/core/layout`): the box tree built from the DOM and
//! each element's `ComputedStyle`, laid out by taffy's block, flex and grid
//! algorithms (with parley for inline content, and the box tree's own table
//! layout) into an immutable fragment tree.
//!
//! The box tree is plain Rust data off the GC heap (unlike Blink's
//! `LayoutObject`s): boxes refer to their elements by `NodeId`, so layout
//! needs no scope once the tree is built. taffy runs through its low-level
//! traits on the box tree itself, not a `TaffyTree`.
//!
//! Block containers hold either block boxes or inline content, never both:
//! inline content next to blocks is wrapped in an anonymous block (CSS 2
//! §9.2.1.1), and a block inside an inline element splits it. Inline
//! elements are no boxes in the box tree: their start and end are items in
//! the inline content, and layout gives them a fragment on each line. An
//! atomic inline (an `inline-block`, an `<img>`) is a box, a child of the
//! block whose inline content it is in, which the lines place (Blink's
//! `LayoutBlockFlow` and `LayoutImage` as atomic inlines).
//!
//! A flex or grid container (`LayoutFlexibleBox`, `LayoutGrid`) holds only
//! block boxes, its items: its children are blockified by style, and each
//! run of text is wrapped in an anonymous item. The box tree keeps them in
//! document order; layout takes them, and makes their fragments, in
//! `order`-modified document order (Blink's `FlexChildIterator`, and the
//! grid's `GridItems`).
//!
//! A table (`LayoutTable`) holds its captions and row groups, a row group
//! its rows, a row its cells; what is missing around content is made
//! anonymous, and table parts out of a table go into an anonymous one
//! (the `table` module). The table formatting context is the box tree's,
//! a leaf to taffy, as an inline formatting context is.
//!
//! The box tree follows the flat tree (`dom`'s `flat_tree_children`;
//! Blink's `LayoutTreeBuilderTraversal`): a shadow host's boxes hold its
//! shadow tree's content, a slot's place holds what is assigned to it (or
//! its fallback), and a `display: contents` element (a slot, by default)
//! has no box, its children being laid out as its parent's.
//!
//! After the first build, [`BoxTree::update`] changes only what the
//! DOM's layout tree flags lead to (Blink's layout tree rebuild): a box
//! takes a new style in place, or the block box a change is in collects its
//! content again, keeping the block boxes in it that didn't change.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

mod calc;
mod canvas_text;
mod caret;
mod clip_path;
mod control;
mod counters;
mod font_families;
mod font_features;
mod fragment;
mod geometry;
mod list_items;
mod platform_fonts;
mod replaced;
mod scroll_anchor;
mod sticky;
mod svg;
pub mod svg_dom;
mod svg_geometry;
mod svg_gradient;
mod svg_text;
mod table;
mod text;
mod tree_builder;

use std::sync::Arc;

use computed_style::{
    BorderCollapse, BoxSizing, Clear, ComputedStyle, ContentAlignment, Display, FlexBasis,
    FlexDirection, FlexWrap, Float, GridAutoFlow, GridLine, GridTemplateAreas, GridTrack,
    ItemAlignment, LengthPercentage, LengthPercentageOrAuto, LengthPercentageOrNone, LineNames,
    Overflow, Position, PseudoId, RepeatCount, Sides, Size, TableDisplay, TextOverflow,
    TrackBreadth, TrackList, TrackSize,
};
use gfx::{NodeId, NodeIdHashMap, NodeIdHashSet, PointF, SizeF};

use crate::calc::CalcValues;
use taffy::{
    AvailableSpace, AxisStaticPosition, BlockContext, BlockFormattingContext, Cache, CacheTree,
    DetailedGridInfo, DetailedLayoutInfo, FloatIntrinsicWidthCalculator, Layout,
    LayoutBlockContainer, LayoutContainingBlock, LayoutFlexboxContainer, LayoutGridContainer,
    LayoutInput, LayoutOutput, LayoutPartialTree, MaybeMath, MaybeResolve, OofCandidate,
    OofCandidates, OofPositioningArea, RequestedAxis, ResolveOrZero, RunMode, SizingMode,
    TraversePartialTree, compute_block_layout, compute_cached_layout, compute_flexbox_layout,
    compute_grid_layout, compute_hidden_layout, compute_leaf_layout, compute_oof_layout,
    compute_root_layout,
};

pub use canvas_text::shape_canvas_text;
pub use caret::first_caret_position;
pub use clip_path::ClipPaths;
pub use control::empty_caret_metrics;
pub use fragment::{
    AtomicInline, BoxFragment, BoxRole, CollapsedBorder, CollapsedBorders, ControlState,
    FragmentLink, InlineBoxFragment, LineEnd, LineEndKind, LineFragment, OutOfFlowPlace,
    TableColumn, TextCluster, TextFragment,
};
pub use geometry::{
    BoxQuads, Quad, ScrollOffsets, border_quads, bounding_rect, box_quads, box_transform,
    client_rects, element_box, is_fixed, pseudo_box_quads, text_rects,
};
pub use platform_fonts::{PlatformFont, platform_fonts};
use replaced::Replaced;
pub use replaced::{ImageState, Images};
pub use scroll_anchor::{ScrollAnchorNode, find_scroll_anchor, scroll_anchor_offset};
pub use sticky::{
    StickyConstraints, StickyKey, StickyOffsets, StickyPositions, StickyReaches, StickyState,
    sticks,
};
pub use svg::{SvgFragment, SvgNode, SvgNodeKind, SvgShape};
pub use svg::{SvgNaturalSize, svg_image_fragment, svg_natural_size};
pub use svg_geometry::{svg_geometry, svg_node, svg_shape_path, svg_text_chars};
pub use svg_gradient::{Spread, SvgGradient, SvgGradientKind, SvgPaintServer};
pub use svg_text::{SvgText, SvgTextCell, SvgTextRun};
pub use text::TextContext;
use text::{
    AtomicInlines, AtomicLayout, Edge, FloatBand, FloatBands, InlineContent, InlineItem, PassedUp,
    relative_offset,
};
pub use tree_builder::svg_element;

/// The `LayoutView`'s index: the initial containing block, sized to the
/// viewport, whose only child is the root element's box.
const LAYOUT_VIEW: usize = 0;

/// The box tree (`LayoutObject` tree) of one document.
pub struct BoxTree {
    /// Indexed by taffy node id; `LAYOUT_VIEW` first.
    boxes: Vec<LayoutBox>,
    /// Indexes in `boxes` no longer in the tree, for the next new boxes.
    unused: Vec<usize>,
    /// Each element's box, block-level or an atomic inline (Blink's
    /// `Node::GetLayoutObject`): what an update keeps or changes in place.
    elements: NodeIdHashMap<usize>,
    /// The box whose inline content each text node with text left after
    /// white space processing, and each `<br>`, is in (Blink's
    /// `Node::GetLayoutObject` for a `LayoutText`): what a selection goes
    /// over. Taken out with the box or its content: debug builds check it
    /// against the box.
    texts: NodeIdHashMap<usize>,
    document: DocumentBoxes,
    last_layout: LayoutUpdate,
    /// The `calc()` values the boxes' taffy styles refer to.
    calcs: CalcValues,
    /// The document's counters, as the boxes show them.
    counters: counters::Counters,
    /// The lists' numbers, worked out as markers show them (while
    /// `counters.lists`).
    lists: list_items::ListOrdinals,
    /// The text each list item's marker shows, as it was built.
    marker_texts: NodeIdHashMap<Arc<str>>,
    /// How many times an update worked the counters out again (for
    /// tests).
    counter_passes: usize,
    /// Whether the block container being laid out is only having its
    /// width measured: its tables are seen without their widths then
    /// (see [`LayoutBox::block_item_style`]).
    measuring_width: bool,
    /// What style changes since last taken mean for scroll anchoring.
    scroll_anchoring: ScrollAnchoringChanges,
    /// The elements whose boxes draw with what they found by id
    /// (an outermost `<svg>`'s content, a box's `clip-path: url()`), with
    /// those ids: made again when one finds another element, or what one
    /// found changed (Blink's `SVGResources`' clients).
    svg_clients: NodeIdHashMap<svg_gradient::Lookups>,
}

/// What the whole box tree depends on: an update builds everything again
/// when it changes.
#[derive(Debug, PartialEq)]
struct DocumentBoxes {
    root: Option<NodeId>,
    /// "The HTML `body` element", if there is one.
    body: Option<NodeId>,
    /// The element whose `overflow` went to the viewport: its own is
    /// `visible` (see [`BoxTree::build`]).
    propagated: Option<NodeId>,
    viewport_overflow: (Overflow, Overflow),
}

struct LayoutBox {
    node: Option<NodeId>,
    /// The pseudo-element of `node` that generated it, if one did: built
    /// again whenever the content it is in is collected again, and not in
    /// [`BoxTree::elements`].
    pseudo: Option<PseudoId>,
    role: BoxRole,
    style: Arc<ComputedStyle>,
    taffy_style: taffy::Style,
    /// An in-flow table's style as its block container sees it while
    /// only the container's width is measured: without its widths, which
    /// taffy would take for its contribution to the container's intrinsic
    /// widths though the table is never narrower than its content (it
    /// resolves them itself). Laid out, the container sees them: they
    /// say whether the table fits beside floats.
    block_item_style: Option<Box<taffy::Style>>,
    /// In document order.
    children: Vec<taffy::NodeId>,
    /// A flex container's children in `order`-modified document order,
    /// sorted when it is laid out; `None` when that is document order.
    ordered: Option<Vec<taffy::NodeId>>,
    /// The out-of-flow boxes this one is the containing block of, in
    /// document order, as its last layout hoisted them here from wherever
    /// they are below it (Blink's `OutOfFlowLayoutPart`): their parent's
    /// layout works out their static position and passes them up, and
    /// taffy lays them out against this box after its own content.
    absolute: Vec<taffy::NodeId>,
    /// For an atomic inline, the out-of-flow boxes its last layout passed
    /// up, unclaimed: the line it is on passes them on, from where it put
    /// it (taffy's layouts pass theirs up themselves).
    oof_candidates: OofCandidates,
    /// Whether its last layout put floats into the block formatting
    /// context it shares with its parent: taffy's cache wouldn't put them
    /// there again.
    places_floats: bool,
    /// A grid container's tracks and items as its last layout placed them:
    /// an out-of-flow child it is the containing block of goes in its grid
    /// area.
    grid_info: DetailedLayoutInfo<String>,
    /// Set if this box is an inline formatting context's root; then its
    /// children are the atomic inlines in it.
    inline: Option<InlineContent>,
    /// Set for a replaced element (`<img>`), block-level or an atomic
    /// inline; it has no children.
    replaced: Option<Replaced>,
    /// Set for a form control with a size or a baseline of its own.
    control: Option<control::Control>,
    /// A button's (HTML's button layout, `control::is_button`).
    button: bool,
    /// An item of a `-webkit-box`: its taffy style has `webkit_box_item`'s
    /// changes, made again without them when it is no longer one.
    webkit_box_item: bool,
    /// A row of a list box (`control::is_list_box_row`): its height is one
    /// the list box's rows are as high as.
    list_box_row: bool,
    /// A text control's inner editor (`control::empty_line_height`): at
    /// least this line high, with text or not.
    empty_line: Option<f32>,
    /// A text control's placeholder (`control::is_placeholder`): the
    /// empty inner editor after it has no line of its own.
    placeholder: bool,
    /// A list item's outside marker (Blink's `LayoutOutsideListMarker`):
    /// not one of its children, which taffy lays out, but laid out with
    /// it as an atomic inline is and put before its first line.
    marker: Option<taffy::NodeId>,
    /// A table cell's columns and rows: `<td colspan rowspan>`'s, 1 and 1
    /// for another cell; a rowspan of 0 goes to the end of its row group.
    /// A column's or column group's columns: `<col span>`'s and
    /// `<colgroup span>`'s, 1 for another.
    span: (u32, u32),
    /// How far down a table cell's content is moved by its
    /// `vertical-align`, as its table placed it: its in-flow children's
    /// locations have it, and its lines are put there. 0 again whenever
    /// it is laid out (Blink aligns a cell's content as it lays it out).
    content_shift: f32,
    /// A table's baseline, from its border box's top, as its last layout
    /// found it: its first row's (css-tables 3).
    table_baseline: Option<f32>,
    /// A table's column groups and columns where its last layout put them.
    table_columns: Vec<TableColumn>,
    /// A table row or cell its table's last layout hid (see
    /// [`BoxFragment::hidden`]).
    hidden: bool,
    /// A box after the clamped line of the line clamp container it is in,
    /// hidden the same way (`BoxTree::clamp_lines`).
    clamped: bool,
    /// With collapsed borders, a cell's border widths (in its taffy style
    /// too, which its table set), or a table's (in its fragment; its
    /// padding is 0).
    collapsed_border: Option<Sides<f32>>,
    /// A table's collapsed borders where its last layout put them.
    collapsed_borders: Option<Arc<CollapsedBorders>>,
    /// For a box with a native look, the states it shows, as the DOM last
    /// had them.
    control_state: Option<ControlState>,
    /// For a range's thumb, where its value is between its minimum and
    /// maximum (0 to 1), which places it in its track.
    slider_ratio: Option<f32>,
    /// See [`BoxFragment::background_inside_border`].
    background_inside_border: bool,
    cache: Cache,
    layout: Layout,
    /// The box this one is a child of; `None` for the `LayoutView`.
    parent: Option<usize>,
    /// Its taffy cache, and its ancestors', was cleared for layout
    /// (`NeedsLayout`); cleared again once laid out.
    needs_layout: bool,
    /// Its fragment, or one below it, has to be made again: laid out
    /// again, or a new style.
    needs_fragment: bool,
    /// The fragment made at the last layout, kept while nothing above
    /// says otherwise (Blink's cached layout result).
    fragment: Option<Arc<BoxFragment>>,
    /// The `<clipPath>` its `clip-path: url()` names, as captured when the
    /// box was built (a change to it builds the box again).
    clip_source: Option<Arc<svg::ClipSource>>,
}

impl LayoutBox {
    fn new(
        node: Option<NodeId>,
        role: BoxRole,
        style: Arc<ComputedStyle>,
        calcs: &CalcValues,
    ) -> Self {
        let taffy_style = role_taffy_style(&style, role, calcs);
        Self {
            node,
            pseudo: None,
            role,
            block_item_style: block_item_style(&style, &taffy_style),
            taffy_style,
            style,
            children: Vec::new(),
            ordered: None,
            absolute: Vec::new(),
            oof_candidates: OofCandidates::NONE,
            places_floats: false,
            grid_info: DetailedLayoutInfo::None,
            inline: None,
            replaced: None,
            control: None,
            button: false,
            list_box_row: false,
            empty_line: None,
            placeholder: false,
            marker: None,
            span: (1, 1),
            content_shift: 0.0,
            table_baseline: None,
            table_columns: Vec::new(),
            hidden: false,
            clamped: false,
            webkit_box_item: false,
            collapsed_border: None,
            collapsed_borders: None,
            control_state: None,
            slider_ratio: None,
            background_inside_border: false,
            cache: Cache::new(),
            layout: Layout::new(),
            parent: None,
            needs_layout: false,
            needs_fragment: true,
            fragment: None,
            clip_source: None,
        }
    }
}

/// What [`BoxTree::build`] and [`BoxTree::update`] did, for
/// tests and `RUST_LOG=layout=debug`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BoxTreeUpdate {
    /// Everything was built: the first time, or the root element, `<body>`
    /// or what goes to the viewport changed.
    pub full: bool,
    /// Boxes built, anonymous blocks included.
    pub built: usize,
    /// Inline formatting contexts shaped (with something in them).
    pub shaped: usize,
    /// Boxes and inline elements that took a new style in place.
    pub restyled: usize,
    /// Outermost `<svg>`s whose content an update captured again (one
    /// built is in `built`).
    pub svg_captured: usize,
    /// Font family names the shaped inline formatting contexts gave
    /// parley: each name of each span's list, aliases and Chrome's
    /// metric-compatible fonts included. fontique looks each one up by
    /// name.
    pub font_family_names: usize,
}

/// What [`BoxTree::layout`] did, for tests and `RUST_LOG=layout=debug`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayoutUpdate {
    /// Boxes laid out, not taken from taffy's cache.
    pub laid_out: usize,
    /// Fragments made, not kept from the last layout.
    pub fragments: usize,
    /// Inline formatting contexts broken into lines, not kept from the
    /// last break at the same width beside the same floats.
    pub lines_broken: usize,
    /// Of `lines_broken`, those broken again at their final width as their
    /// fragments were made: their last break was at another width or
    /// beside other floats (taffy measured them since), and parley keeps
    /// only the last.
    pub lines_broken_for_fragments: usize,
    /// Lines beside floats broken again from their start: lower down where
    /// there is more room, in less room once taller, without the floats
    /// that turned out on the next line, or beside the floats placed at
    /// their top.
    pub lines_broken_again_beside_floats: usize,
    /// Of `laid_out`, the boxes laid out without taffy's cache because of
    /// floats: next to some, or putting their own into the block
    /// formatting context they share (the cache keys on its inputs alone).
    pub laid_out_beside_floats: usize,
    /// Text clusters and glyphs placed (`text-overflow` needs them): the
    /// rest are placed when asked, by paint and hit testing.
    pub clusters_placed: usize,
    pub glyphs_placed: usize,
}

/// How a new style differs from the one a box has (Blink's
/// `StyleDifference`), from the most to the least work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleDifference {
    /// Other boxes: built again (`NeedsReattachLayoutTree`).
    Reattach,
    /// The same boxes, laid out again.
    Layout,
    /// Only what is painted, if anything: style recalc flags only other
    /// values, and those that differ only in what goes to the viewport
    /// build everything again.
    Paint,
}

/// What style changes since the box tree last said mean for scroll
/// anchoring, which the renderer does.
#[derive(Debug, Default)]
pub struct ScrollAnchoringChanges {
    /// The elements an anchor in which, or which is, isn't adjusted for:
    /// css-scroll-anchoring's suppression triggers, checked on the way from
    /// an anchor up to its scroller.
    pub disabled: Vec<NodeId>,
    /// The elements whose `overflow-anchor` changed: the scrollers around
    /// them pick their anchors again (Blink's `ClearAncestorScrollAnchors`).
    pub overflow_anchor: Vec<NodeId>,
    /// The elements whose boxes were built again, with what is in them:
    /// an anchor there is gone, and none is picked there before the
    /// layout (Blink destroys their layout objects; the new ones haven't
    /// been laid out). Not when the whole tree is built again, which keeps
    /// the anchors.
    pub rebuilt: Vec<NodeId>,
}

/// Whether a box's new style changes what css-scroll-anchoring's
/// suppression triggers name, which keeps a scroll anchor in it from being
/// adjusted for (Blink's `ComputedStyle::
/// ScrollAnchorDisablingPropertyChanged`): its offsets, margins, padding,
/// sizes, `position` or `transform`.
fn disables_scroll_anchoring(old: &ComputedStyle, new: &ComputedStyle) -> bool {
    old.inset != new.inset
        || old.margin != new.margin
        || old.padding != new.padding
        || old.width != new.width
        || old.height != new.height
        || old.min_width != new.min_width
        || old.min_height != new.min_height
        || old.max_width != new.max_width
        || old.max_height != new.max_height
        || old.position != new.position
        || old.transform != new.transform
}

impl StyleDifference {
    fn between(old: &ComputedStyle, new: &ComputedStyle) -> Self {
        // Whether the box is in the flow decides the shape of the tree:
        // an out-of-flow one or a float is blockified, and its parent's
        // content is collected again.
        if old.display != new.display
            || old.is_out_of_flow() != new.is_out_of_flow()
            || (old.float == Float::None) != (new.float == Float::None)
        {
            return Self::Reattach;
        }
        // A `<clipPath>` is captured as the box is built.
        let url = |style: &ComputedStyle| {
            matches!(style.rare.clip_path, computed_style::ClipPath::Url(_))
        };
        if (url(old) || url(new)) && old.rare.clip_path != new.rare.clip_path {
            return Self::Reattach;
        }
        // The new style with the old values of what only painting uses:
        // the same as the old, and nothing else changed.
        let layout = ComputedStyle {
            border_style: old.border_style,
            border_color: old.border_color,
            // The rare group's paint-only values from the old: the rest
            // of the group compares as the new's.
            rare: if new.rare.border_radius == old.rare.border_radius
                && new.rare.transform_origin == old.rare.transform_origin
                && new.rare.transform_box == old.rare.transform_box
                && new.rare.clip == old.rare.clip
                && new.rare.clip_path == old.rare.clip_path
                && new.rare.text_decoration_style == old.rare.text_decoration_style
                && new.rare.text_decoration_color == old.rare.text_decoration_color
                && new.rare.text_decoration_thickness == old.rare.text_decoration_thickness
                && new.rare.filter == old.rare.filter
                && new.rare.isolation == old.rare.isolation
                && new.rare.resize == old.rare.resize
                && new.rare.scroll_behavior == old.rare.scroll_behavior
                && new.rare.overflow_anchor == old.rare.overflow_anchor
                && new.rare.border_image == old.rare.border_image
                && new.rare.highlight_variables == old.rare.highlight_variables
            {
                new.rare.clone()
            } else {
                std::sync::Arc::new(computed_style::RareNonInherited {
                    border_radius: old.rare.border_radius,
                    transform_origin: old.rare.transform_origin,
                    transform_box: old.rare.transform_box,
                    clip: old.rare.clip,
                    clip_path: old.rare.clip_path.clone(),
                    text_decoration_style: old.rare.text_decoration_style,
                    text_decoration_color: old.rare.text_decoration_color,
                    text_decoration_thickness: old.rare.text_decoration_thickness,
                    filter: old.rare.filter.clone(),
                    isolation: old.rare.isolation,
                    // Parsed only: nothing drawn or laid out differs.
                    resize: old.rare.resize,
                    scroll_behavior: old.rare.scroll_behavior,
                    // Only which box anchors scrolling: the renderer's.
                    overflow_anchor: old.rare.overflow_anchor,
                    border_image: old.rare.border_image.clone(),
                    // What `getComputedStyle` reads of its `::selection`.
                    highlight_variables: old.rare.highlight_variables.clone(),
                    ..(*new.rare).clone()
                })
            },
            box_shadow: old.box_shadow.clone(),
            outline_width: old.outline_width,
            outline_style: old.outline_style,
            outline_color: old.outline_color,
            outline_offset: old.outline_offset,
            background_color: old.background_color,
            backgrounds: old.backgrounds.clone(),
            text_decoration_line: old.text_decoration_line,
            opacity: old.opacity,
            transform: old.transform.clone(),
            z_index: old.z_index,
            effective_z_index: old.effective_z_index,
            color: old.color,
            // The font group's paint-only value (parsed only), likewise.
            font: if new.font.smoothing == old.font.smoothing {
                new.font.clone()
            } else {
                std::sync::Arc::new(computed_style::FontDescription {
                    smoothing: old.font.smoothing,
                    ..(*new.font).clone()
                })
            },
            // The rare inherited group's paint-only values, likewise.
            rare_inherited: if new.rare_inherited.applied_text_decorations
                == old.rare_inherited.applied_text_decorations
                && new.rare_inherited.text_shadow == old.rare_inherited.text_shadow
                && new.rare_inherited.text_fill_color == old.rare_inherited.text_fill_color
                && new.rare_inherited.text_underline_offset
                    == old.rare_inherited.text_underline_offset
                && new.rare_inherited.text_underline_position
                    == old.rare_inherited.text_underline_position
                && new.rare_inherited.user_select == old.rare_inherited.user_select
                && new.rare_inherited.user_modify == old.rare_inherited.user_modify
                && new.rare_inherited.cursor == old.rare_inherited.cursor
                && new.rare_inherited.forced_color_adjust == old.rare_inherited.forced_color_adjust
                && new.rare_inherited.tap_highlight_color == old.rare_inherited.tap_highlight_color
                && new.rare_inherited.text_size_adjust == old.rare_inherited.text_size_adjust
                && new.rare_inherited.caret_color == old.rare_inherited.caret_color
                && new.rare_inherited.highlight == old.rare_inherited.highlight
            {
                new.rare_inherited.clone()
            } else {
                std::sync::Arc::new(computed_style::RareInherited {
                    applied_text_decorations: old.rare_inherited.applied_text_decorations.clone(),
                    text_shadow: old.rare_inherited.text_shadow.clone(),
                    text_fill_color: old.rare_inherited.text_fill_color,
                    text_underline_offset: old.rare_inherited.text_underline_offset,
                    text_underline_position: old.rare_inherited.text_underline_position,
                    // Parsed only: nothing drawn or laid out differs.
                    user_select: old.rare_inherited.user_select,
                    user_modify: old.rare_inherited.user_modify,
                    // Only what the pointer looks like.
                    cursor: old.rare_inherited.cursor.clone(),
                    forced_color_adjust: old.rare_inherited.forced_color_adjust,
                    tap_highlight_color: old.rare_inherited.tap_highlight_color,
                    text_size_adjust: old.rare_inherited.text_size_adjust,
                    // What the caret and a selection are drawn in.
                    caret_color: old.rare_inherited.caret_color,
                    highlight: old.rare_inherited.highlight.clone(),
                    ..(*new.rare_inherited).clone()
                })
            },
            // A sticky box is laid out unmoved: its inset is read after
            // layout, for how it moves as its scrollport scrolls.
            inset: if old.position == Position::Sticky && new.position == Position::Sticky {
                old.inset
            } else {
                new.inset
            },
            // What hit testing's regions take from paint.
            inside_blocking_wheel_event_handler: old.inside_blocking_wheel_event_handler,
            // What the style phase keeps for its own invalidation.
            has_viewport_units: old.has_viewport_units,
            explicit_inheritance: old.explicit_inheritance,
            ..new.clone()
        };
        if layout == *old {
            Self::Paint
        } else {
            Self::Layout
        }
    }
}

/// What giving boxes their content needs besides the tree (`set_content`
/// and the table's parts).
struct ContentContext<'a> {
    text: &'a mut TextContext,
    /// Inside a line clamp container: inline content here may end with
    /// the clamp's ellipsis, so it is shaped ahead.
    in_line_clamp: bool,
    stats: BoxTreeUpdate,
}

/// The style `element`'s box goes by: `style`, but for an `<input>`
/// button, whose label has the button layout whatever its `display`
/// (Blink's `BaseButtonInputType` makes a `LayoutButton`): a flex or grid
/// one is laid out as a block container (its computed value stays).
fn box_style(
    element: control::ControlElement<'_, '_>,
    style: Arc<ComputedStyle>,
) -> Arc<ComputedStyle> {
    let display = match style.display {
        Display::Flex | Display::Grid | Display::WebkitBox => Display::Block,
        Display::InlineFlex | Display::InlineGrid | Display::WebkitInlineBox => {
            Display::InlineBlock
        }
        _ => return style,
    };
    if !control::is_input_button(element) {
        return style;
    }
    Arc::new(ComputedStyle {
        display,
        ..(*style).clone()
    })
}

/// `style` with `overflow: visible`, for the element whose `overflow` went
/// to the viewport.
fn visible_overflow(style: &ComputedStyle) -> Arc<ComputedStyle> {
    Arc::new(ComputedStyle {
        overflow_x: Overflow::Visible,
        overflow_y: Overflow::Visible,
        ..style.clone()
    })
}

/// taffy's style for a block-level replaced element; `container` is the
/// style of the flex or grid container it is an item of, if it is one.
///
/// An item keeps its own sizes: the leaf measures it to its natural
/// size, or through its natural ratio from the size it is given
/// ([`Replaced::measure`]), which the flex and grid algorithms need for the
/// item's automatic minimum size and to carry a stretched size through the
/// ratio.
/// Not taffy's `aspect_ratio`, which it applies to the border box. A grid
/// item with a natural size isn't stretched where its alignment is
/// `normal` (css-grid 2, "Grid Item Sizing"), which taffy does for an item
/// it knows is replaced.
///
/// Elsewhere it has the size filled in: taffy's block layout would
/// otherwise stretch an `auto` width to the containing block. A pixel size
/// is converted to the content box; a percentage width stays a percentage
/// (and `box-sizing` is then not taken into account), with an `auto`
/// height following it through taffy's `aspect_ratio` if there is a
/// natural ratio. Only then: taffy applies the ratio to the border box.
fn replaced_taffy_style(
    style: &ComputedStyle,
    replaced: &Replaced,
    container: Option<&ComputedStyle>,
    calcs: &CalcValues,
) -> taffy::Style {
    use taffy::Dimension;

    if let Some(container) = container {
        let mut taffy_style = taffy_style(style, calcs);
        taffy_style.item_is_replaced =
            container.display.is_grid() && replaced.natural_size().is_some();
        return taffy_style;
    }
    let mut taffy_style = taffy_style(style, calcs);
    taffy_style.box_sizing = taffy::BoxSizing::ContentBox;
    // Only a natural ratio: its margin box the containing block's width
    // (CSS 2 § 10.3.3's `auto` width, as a block's), the height through
    // the ratio (`Replaced::fills_width`).
    if replaced.fills_width(style) {
        let (mut px, mut percent) = (-(style.border_width.left + style.border_width.right), 100.0);
        let mut leave = |value: LengthPercentage| match value {
            LengthPercentage::Px(v) => px -= v,
            LengthPercentage::Percent(p) => percent -= p,
            LengthPercentage::Calc {
                px: v, percent: p, ..
            } => {
                px -= v;
                percent -= p;
            }
        };
        for margin in [style.margin.left, style.margin.right] {
            if let LengthPercentageOrAuto::LengthPercentage(margin) = margin {
                leave(margin);
            }
        }
        leave(style.padding.left);
        leave(style.padding.right);
        taffy_style.size.width = Dimension::calc(calcs.handle(px, percent, true));
        taffy_style.size.height = Dimension::auto();
        taffy_style.aspect_ratio = replaced.ratio();
        return taffy_style;
    }
    // What the element would be with no containing block: pixel sizes and
    // the natural ones, before `min-*` and `max-*`, which taffy applies
    // with percentages of the containing block it knows (a `max-width:
    // 100%` resolved against no width here would be 0).
    let known = replaced.unclamped_content_size(style, Some(0.0));
    let ratio = replaced.ratio();
    // The intrinsic size keywords are its natural size, as `auto`.
    // A percentage (or a `calc()` with one) stays as `taffy_style` has it.
    taffy_style.size.width = match style.width {
        Size::LengthPercentage(v) if v.has_percent() => taffy_style.size.width,
        _ => Dimension::length(known.width),
    };
    let height_is_px = matches!(
        style.height,
        Size::LengthPercentage(LengthPercentage::Px(_))
    );
    let width_is_percent = matches!(style.width, Size::LengthPercentage(v) if v.has_percent());
    // `min-width` or `max-width` may change the width taffy lays it out at.
    let width_is_clamped = style.max_width != LengthPercentageOrNone::None
        || style.min_width != LengthPercentageOrAuto::Auto
            && style.min_width
                != LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(0.0));
    // An `auto` height follows the width through the ratio where the width
    // isn't known here: a percentage, or what `max-width` and `min-width`
    // leave of it (CSS 2 § 10.4's table: a `max-width: 100%` image or
    // `<svg>` shrinks keeping its shape).
    taffy_style.size.height = match ratio {
        Some(ratio) if !height_is_px && (width_is_percent || width_is_clamped) => {
            taffy_style.aspect_ratio = Some(ratio);
            Dimension::auto()
        }
        _ => Dimension::length(known.height),
    };
    taffy_style
}

/// A block container's content, in order, with inline elements flattened.
enum Child {
    Block(taffy::NodeId),
    Inline(InlineItem),
}

impl BoxTree {
    /// What the style changes since last taken mean for scroll anchoring.
    pub fn take_scroll_anchoring_changes(&mut self) -> ScrollAnchoringChanges {
        std::mem::take(&mut self.scroll_anchoring)
    }

    /// A tree of only the `LayoutView`, for what `document` says of the
    /// whole document's boxes.
    fn empty(document: DocumentBoxes) -> Self {
        let view_style = ComputedStyle {
            display: Display::Block,
            ..ComputedStyle::initial()
        };
        let calcs = CalcValues::default();
        Self {
            boxes: vec![LayoutBox::new(
                None,
                BoxRole::LayoutView,
                Arc::new(view_style),
                &calcs,
            )],
            unused: Vec::new(),
            elements: NodeIdHashMap::default(),
            texts: NodeIdHashMap::default(),
            document,
            last_layout: LayoutUpdate::default(),
            calcs,
            counters: counters::Counters::default(),
            lists: list_items::ListOrdinals::default(),
            marker_texts: NodeIdHashMap::default(),
            counter_passes: 0,
            measuring_width: false,
            scroll_anchoring: ScrollAnchoringChanges::default(),
            svg_clients: NodeIdHashMap::default(),
        }
    }

    /// Takes the counters' texts, by element and pseudo-element, and the
    /// elements that take part (`set_counters`).
    fn set_counter_texts(
        &mut self,
        texts: rustc_hash::FxHashMap<(NodeId, PseudoId), Arc<str>>,
        relevant: NodeIdHashMap<counters::Directives>,
        lists: bool,
    ) {
        self.counters = counters::Counters {
            texts,
            relevant,
            lists,
        };
        if !lists {
            self.lists = list_items::ListOrdinals::default();
        }
    }

    /// Grid `id`'s size measured in `inputs` as if it had no items: its
    /// explicit tracks and its edges.
    fn empty_grid_size(&mut self, id: taffy::NodeId, inputs: LayoutInput) -> taffy::Size<f32> {
        let index = usize::from(id);
        let ordered = self.boxes[index].ordered.replace(Vec::new());
        let measured = compute_grid_layout(
            self,
            id,
            LayoutInput {
                run_mode: RunMode::ComputeSize,
                ..inputs
            },
        );
        self.boxes[index].ordered = ordered;
        measured.size
    }

    /// Whether there is an out-of-flow box among box `index`'s
    /// descendants.
    fn has_out_of_flow_below(&self, index: usize) -> bool {
        let mut stack = self.boxes[index].children.clone();
        while let Some(child) = stack.pop() {
            let b = &self.boxes[usize::from(child)];
            if b.style.is_out_of_flow() {
                return true;
            }
            stack.extend(b.children.iter().copied());
        }
        false
    }

    /// Whether one of box `index`'s children is an anonymous table box.
    fn has_anonymous_table_box(&self, index: usize) -> bool {
        self.boxes[index]
            .children
            .iter()
            .any(|&child| self.boxes[usize::from(child)].role == BoxRole::AnonymousTable)
    }

    /// Box `index` is laid out again, and so are its ancestors
    /// (`SetNeedsLayout` with `MarkContainerChainForLayout`): their taffy
    /// caches are cleared, up to the first marked already.
    fn mark_needs_layout(&mut self, index: usize) {
        // An out-of-flow box is laid out by its containing block, which
        // this walk reaches: the containing block is one of its ancestors.
        let mut next = Some(index);
        while let Some(index) = next {
            let b = &mut self.boxes[index];
            if b.needs_layout {
                return;
            }
            // Laid out again with its cache gone, it makes its fragment
            // again too.
            b.needs_layout = true;
            b.cache.clear();
            // An atomic inline in it may take other room.
            if let Some(inline) = &mut b.inline {
                inline.atomics_changed();
            }
            next = b.parent;
        }
    }

    /// Box `index` makes its fragment again (a new style), and so do its
    /// ancestors, to hold it.
    fn mark_needs_fragment(&mut self, index: usize) {
        let mut next = Some(index);
        while let Some(index) = next {
            let b = &mut self.boxes[index];
            if b.needs_fragment {
                return;
            }
            b.needs_fragment = true;
            next = b.parent;
        }
    }

    /// The boxes holding box `index`'s inline content: itself, if it is an
    /// inline formatting context, or its anonymous blocks.
    fn contents(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let b = &self.boxes[index];
        b.inline.is_some().then_some(index).into_iter().chain(
            b.children
                .iter()
                .map(|&child| usize::from(child))
                .filter(|&child| self.boxes[child].role == BoxRole::AnonymousBlock),
        )
    }

    /// Puts `b` into an unused index, or a new one.
    fn allocate(&mut self, b: LayoutBox) -> usize {
        match self.unused.pop() {
            Some(index) => {
                self.boxes[index] = b;
                index
            }
            None => {
                let index = self.boxes.len();
                self.boxes.push(b);
                index
            }
        }
    }

    /// Indexes the text nodes and `<br>`s in box `index`'s inline content
    /// (`BoxTree::texts`).
    fn index_texts(&mut self, index: usize) {
        if let Some(inline) = &self.boxes[index].inline {
            self.texts.extend(inline.nodes().map(|node| (node, index)));
        }
    }

    /// Takes those in box `index`'s inline content out of the index, before
    /// it goes.
    fn forget_texts(&mut self, index: usize) {
        if let Some(inline) = &self.boxes[index].inline {
            for node in inline.nodes() {
                if self.texts.get(&node) == Some(&index) {
                    self.texts.remove(&node);
                }
            }
        }
    }

    /// Box `index` and its descendants are no longer in the tree.
    fn release(&mut self, index: usize) {
        let empty = LayoutBox::new(
            None,
            BoxRole::Other,
            self.boxes[LAYOUT_VIEW].style.clone(),
            &self.calcs,
        );
        self.forget_texts(index);
        let b = std::mem::replace(&mut self.boxes[index], empty);
        // Its element may have a new box already.
        if let Some(node) = b.node
            && self.elements.get(&node) == Some(&index)
        {
            self.elements.remove(&node);
        }
        if let Some(marker) = b.marker {
            self.release(marker.into());
        }
        // Not an atomic inline that went to another box with its content.
        for child in b.children {
            if self.boxes[usize::from(child)].parent == Some(index) {
                self.release(child.into());
            }
        }
        self.unused.push(index);
    }

    /// The text the counters gave `node`'s `pseudo` (a list item's
    /// marker's, or one showing a counter), for tests.
    pub fn counter_text_for_testing(&self, node: NodeId, pseudo: PseudoId) -> Option<String> {
        if pseudo == PseudoId::Marker && self.counters.lists {
            return self.marker_texts.get(&node).map(|text| text.to_string());
        }
        self.counters
            .texts
            .get(&(node, pseudo))
            .map(|text| text.to_string())
    }

    /// How many times an update has worked the counters out again, for
    /// tests.
    pub fn counter_passes_for_testing(&self) -> usize {
        self.counter_passes
    }

    /// How many times a list's items were numbered (`list_items`), and
    /// whether the lists number their own, for tests.
    pub fn lists_numbered_for_testing(&self) -> (usize, bool) {
        (self.lists.numbered(), self.counters.lists)
    }

    /// How many boxes the tree has room for, in use or not: for tests that
    /// check an update reuses the room of the boxes it drops.
    /// How many nodes the index of what a selection goes over has
    /// (`BoxTree::texts`), stale ones included.
    pub fn texts_for_testing(&self) -> usize {
        self.texts.len()
    }

    pub fn capacity_for_testing(&self) -> usize {
        self.boxes.len()
    }

    /// Box `index` holds inline `content`: its atomic inlines, out-of-flow
    /// boxes and floats are its children.
    fn adopt_atomics(&mut self, index: usize, content: &[InlineItem]) {
        let children: Vec<taffy::NodeId> = content
            .iter()
            .filter_map(|item| match item {
                InlineItem::Atomic(id, _)
                | InlineItem::OutOfFlow(id, _)
                | InlineItem::Float(id) => Some(*id),
                _ => None,
            })
            .collect();
        for &child in &children {
            self.boxes[usize::from(child)].parent = Some(index);
        }
        self.boxes[index].children = children;
    }

    /// Gives box `index` its content: an inline formatting context if it
    /// is all inline, otherwise block children with each run of inline
    /// content wrapped in an anonymous block. A flex or grid container's is
    /// never inline: each run is an anonymous item (css-flexbox 1, "Flex
    /// Items"; css-grid 2, "Grid Items"). An out-of-flow box or a float
    /// goes with the inline content before it, if there is any that
    /// doesn't collapse away; otherwise among the blocks (Blink's
    /// `LayoutBlockFlow::AddChild`), as it always does in a flex or grid
    /// container. Floats with no other inline content than that, or
    /// out-of-flow boxes, are among the blocks too: Blink lays them out in
    /// an inline formatting context with no lines, the same as the block
    /// layout places them.
    fn set_content(&mut self, cx: &mut ContentContext<'_>, index: usize, content: Vec<Child>) {
        // A table, row group or row: its parts, with the missing ones made.
        if matches!(
            table::TablePart::of(self.boxes[index].style.display),
            Some(table::TablePart::Table | table::TablePart::RowGroup | table::TablePart::Row)
        ) {
            self.set_table_content(cx, index, content);
            return;
        }
        // Table parts out of a table go into an anonymous one.
        let content = self.wrap_misparented(cx, index, content);
        let has_items = self.boxes[index].style.display.is_flexible_or_grid();
        let only_floats = content
            .iter()
            .any(|c| matches!(c, Child::Inline(InlineItem::Float(_))))
            && content.iter().all(|c| match c {
                Child::Inline(InlineItem::Float(_) | InlineItem::OutOfFlow(..)) => true,
                Child::Inline(item) => {
                    InlineItem::is_collapsible_space_only(std::slice::from_ref(item))
                }
                Child::Block(_) => false,
            });
        if !has_items && !only_floats && !content.iter().any(|c| matches!(c, Child::Block(_))) {
            let items: Vec<InlineItem> = content
                .into_iter()
                .map(|c| match c {
                    Child::Inline(item) => item,
                    Child::Block(_) => unreachable!(),
                })
                .collect();
            let style = self.boxes[index].style.clone();
            let editor = self.boxes[index].empty_line.is_some();
            let inline = InlineContent::new(
                cx.text,
                &style,
                &items,
                truncates(&style),
                cx.in_line_clamp,
                editor,
            );
            cx.stats.shaped = cx.stats.shaped.wrapping_add(usize::from(inline.is_some()));
            cx.stats.font_family_names = cx
                .stats
                .font_family_names
                .wrapping_add(cx.text.take_family_names());
            self.boxes[index].inline = inline;
            self.index_texts(index);
            self.adopt_atomics(index, &items);
            return;
        }
        let mut children = Vec::new();
        let mut run = Vec::new();
        // Whether a block box is in the flow before the run: its lines
        // aren't the element's first formatted line then. Floats and
        // out-of-flow boxes don't count (Blink keeps those in its
        // anonymous block with the text), nor do a flex or grid
        // container's items: an anonymous item isn't an anonymous block
        // box, its first line is always indented (Chrome).
        let mut after_block = false;
        for child in content {
            match child {
                Child::Block(id) => {
                    self.anonymous_block(cx, index, &mut run, after_block, &mut children);
                    children.push(id);
                    after_block = !has_items;
                }
                Child::Inline(InlineItem::OutOfFlow(id, _) | InlineItem::Float(id))
                    if has_items || InlineItem::is_collapsible_space_only(&run) =>
                {
                    self.anonymous_block(cx, index, &mut run, after_block, &mut children);
                    children.push(id);
                }
                Child::Inline(item) => run.push(item),
            }
        }
        self.anonymous_block(cx, index, &mut run, after_block, &mut children);
        let container = has_items.then(|| self.boxes[index].style.clone());
        let webkit_box = container
            .as_ref()
            .is_some_and(|c| c.display.is_webkit_box());
        for &child in &children {
            let b = &mut self.boxes[usize::from(child)];
            b.parent = Some(index);
            let sizing = b
                .replaced
                .as_ref()
                .or(b.control.as_ref().and_then(|c| c.sizing.as_ref()));
            if let Some(sizing) = sizing {
                b.taffy_style =
                    replaced_taffy_style(&b.style, sizing, container.as_deref(), &self.calcs);
            } else if b.button {
                b.taffy_style =
                    button_taffy_style(&b.style, b.role, container.is_some(), &self.calcs);
            } else if webkit_box || b.webkit_box_item {
                b.taffy_style = role_taffy_style(&b.style, b.role, &self.calcs);
            }
            if webkit_box && let Some(container) = &container {
                webkit_box_item(&b.style, container, &mut b.taffy_style);
            }
            b.webkit_box_item = webkit_box;
        }
        self.boxes[index].children = children;
    }
}

/// Whether a block container's lines are truncated with an ellipsis: it
/// has `text-overflow: ellipsis` and clips its overflow (css-overflow 4;
/// Blink's `ShouldTruncateOverflowingText`).
fn truncates(style: &ComputedStyle) -> bool {
    style.text_overflow == TextOverflow::Ellipsis && clips_content(style)
}

impl BoxTree {
    /// Counts the text clusters and glyphs that making an inline formatting
    /// context's fragments placed (`LayoutUpdate`).
    fn count_text_placed(&mut self, text: text::TextPlaced) {
        let layout = &mut self.last_layout;
        layout.clusters_placed = layout.clusters_placed.saturating_add(text.clusters);
        layout.glyphs_placed = layout.glyphs_placed.saturating_add(text.glyphs);
    }

    /// Wraps `run` in an anonymous block, unless it is only white space
    /// that would collapse away, or in a flex container, only white space.
    /// Its first line is indented unless it comes `after_block`.
    fn anonymous_block(
        &mut self,
        cx: &mut ContentContext<'_>,
        parent: usize,
        run: &mut Vec<InlineItem>,
        after_block: bool,
        children: &mut Vec<taffy::NodeId>,
    ) {
        let items = std::mem::take(run);
        let empty = if self.boxes[parent].style.display.is_flexible_or_grid() {
            InlineItem::is_white_space_only(&items)
        } else {
            InlineItem::is_collapsible_space_only(&items)
        };
        if items.is_empty() || empty {
            return;
        }
        // Its lines are truncated as the parent's would be (Blink's
        // `ShouldTruncateOverflowingText`).
        let truncates = truncates(&self.boxes[parent].style);
        let style = Arc::new(ComputedStyle::anonymous_block(&self.boxes[parent].style));
        // A text control's inner editor holds only text, never a block:
        // this isn't one.
        let Some(mut inline) =
            InlineContent::new(cx.text, &style, &items, truncates, cx.in_line_clamp, false)
        else {
            return;
        };
        if after_block {
            inline.not_first_formatted_line();
        }
        cx.stats.shaped = cx.stats.shaped.wrapping_add(1);
        cx.stats.font_family_names = cx
            .stats
            .font_family_names
            .wrapping_add(cx.text.take_family_names());
        let mut anonymous = LayoutBox::new(None, BoxRole::AnonymousBlock, style, &self.calcs);
        anonymous.inline = Some(inline);
        cx.stats.built = cx.stats.built.wrapping_add(1);
        let index = self.allocate(anonymous);
        self.index_texts(index);
        self.adopt_atomics(index, &items);
        children.push(index.into());
    }

    /// The viewport's `overflow` on each axis: the root element's, or
    /// `<body>`'s (see [`BoxTree::build`]); `visible` without either.
    pub fn viewport_overflow(&self) -> (Overflow, Overflow) {
        self.document.viewport_overflow
    }

    /// Lays the tree out for a viewport of `viewport` CSS px and returns
    /// the `LayoutView`'s fragment.
    pub fn layout(&mut self, viewport: SizeF) -> Arc<BoxFragment> {
        let _span = trace_event::trace_event!("Layout");
        let size = taffy::Size {
            width: taffy::Dimension::length(viewport.width),
            height: taffy::Dimension::length(viewport.height),
        };
        // Another viewport is another size in for the `LayoutView` and
        // the boxes below, which taffy's cache keys on: those whose result
        // doesn't change are kept.
        self.boxes[LAYOUT_VIEW].taffy_style.size = size;
        self.last_layout = LayoutUpdate::default();
        compute_root_layout(
            self,
            LAYOUT_VIEW.into(),
            taffy::Size {
                width: AvailableSpace::Definite(viewport.width),
                height: AvailableSpace::Definite(viewport.height),
            },
        );
        let fragment = self.fragment(LAYOUT_VIEW.into());
        tracing::debug!(
            laid_out = self.last_layout.laid_out,
            fragments = self.last_layout.fragments,
            lines_broken = self.last_layout.lines_broken,
            lines_broken_for_fragments = self.last_layout.lines_broken_for_fragments,
            lines_broken_again_beside_floats = self.last_layout.lines_broken_again_beside_floats,
            laid_out_beside_floats = self.last_layout.laid_out_beside_floats,
            clusters_placed = self.last_layout.clusters_placed,
            glyphs_placed = self.last_layout.glyphs_placed,
            "layout"
        );
        fragment
    }

    /// What the last [`BoxTree::layout`] did.
    /// The element of the box whose inline content text node or `<br>`
    /// `node` is in (an anonymous block's: the nearest box's around it
    /// with an element), if it is laid out: what paints a caret in it.
    pub fn block_of(&self, node: NodeId) -> Option<NodeId> {
        let mut at = Some(*self.texts.get(&node)?);
        while let Some(index) = at {
            if let Some(element) = self.boxes[index].node {
                return Some(element);
            }
            at = self.boxes[index].parent;
        }
        None
    }

    /// Whether `node` is laid out as what a selection goes over (Blink's
    /// `LayoutSelection` walks only nodes with a layout object): a text
    /// node with text left after white space processing, a `<br>`, or a
    /// replaced element.
    pub fn is_rendered(&self, node: NodeId) -> bool {
        if let Some(&index) = self.texts.get(&node) {
            debug_assert!(
                self.boxes[index]
                    .inline
                    .as_ref()
                    .is_some_and(|inline| inline.nodes().any(|n| n == node)),
                "the index keeps up with the boxes' content"
            );
            return true;
        }
        self.elements
            .get(&node)
            .is_some_and(|&index| self.boxes[index].replaced.is_some())
    }

    pub fn last_layout(&self) -> LayoutUpdate {
        self.last_layout
    }

    /// Box `id`'s fragment: the last one, unless it or one below was laid
    /// out again or took a new style.
    fn fragment(&mut self, id: taffy::NodeId) -> Arc<BoxFragment> {
        let b = &self.boxes[usize::from(id)];
        if !b.needs_fragment
            && let Some(fragment) = &b.fragment
        {
            return fragment.clone();
        }
        let sides = |r: taffy::Rect<f32>| Sides {
            top: r.top,
            right: r.right,
            bottom: r.bottom,
            left: r.left,
        };
        // The out-of-flow boxes it is the containing block of before all:
        // their parents' fragments (below this one) hold theirs, to paint
        // them where they are in tree order, so those are made again with
        // a new one.
        let index = usize::from(id);
        for child in self.boxes[index].absolute.clone() {
            let child = usize::from(child);
            let anew = self.boxes[child].needs_fragment || self.boxes[child].fragment.is_none();
            self.fragment(child.into());
            if anew {
                let mut next = self.boxes[child].parent.filter(|&i| i != index);
                while let Some(i) = next {
                    self.boxes[i].needs_fragment = true;
                    next = self.boxes[i].parent.filter(|&i| i != index);
                }
            }
        }
        // The lines first: they place the atomic inlines, its children.
        let mut lines = if self.boxes[usize::from(id)].inline.is_some() {
            self.inline_fragments(usize::from(id))
        } else {
            Vec::new()
        };
        // In the order they were laid out, which paint and hit testing
        // follow: the in-flow ones, then the out-of-flow ones it is the
        // containing block of (its own out-of-flow children are their
        // containing block's). An inline formatting context's floats come
        // after its atomic inlines, which its lines refer to by their index.
        // A table's columns have no fragments.
        let inline = self.boxes[usize::from(id)].inline.is_some();
        let (floats, in_flow): (Vec<taffy::NodeId>, Vec<taffy::NodeId>) = self
            .layout_children(usize::from(id))
            .iter()
            .copied()
            .filter(|&child| {
                let style = &self.boxes[usize::from(child)].style;
                !style.is_out_of_flow()
                    && !matches!(
                        table::TablePart::of(style.display),
                        Some(table::TablePart::ColumnGroup | table::TablePart::Column)
                    )
            })
            .partition(|&child| {
                inline && self.boxes[usize::from(child)].style.float != Float::None
            });
        let out_of_flow = self.out_of_flow_places(index, &in_flow);
        let children: Vec<taffy::NodeId> = in_flow
            .into_iter()
            .chain(floats)
            .chain(self.boxes[usize::from(id)].absolute.iter().copied())
            .collect();
        let mut children: Vec<FragmentLink> = children
            .into_iter()
            .map(|child| {
                let location = self.boxes[usize::from(child)].layout.location;
                FragmentLink {
                    offset: PointF::new(location.x, location.y),
                    fragment: self.fragment(child),
                }
            })
            .collect();
        // Last, so as not to move the atomic inlines' indexes the lines
        // refer to. With lines of its own, painted and hit with the first
        // as its first atomic inline (only the line places its children),
        // before anything on it.
        if let Some(marker) = self.boxes[usize::from(id)].marker {
            let marker_child = children.len();
            children.push(self.marker_link(usize::from(id), marker));
            if let Some(first) = lines.first_mut() {
                first.atomic_inlines.insert(
                    0,
                    AtomicInline {
                        child: marker_child,
                        paint_before: 0,
                        after_boxes: 0,
                        hidden: false,
                    },
                );
            }
        }
        let b = &mut self.boxes[usize::from(id)];
        let (mut padding, mut border) = (b.layout.padding, b.layout.border);
        // A table with collapsed borders: its own are its edges' (what its
        // parent's layout of it took from its style isn't).
        if table::TablePart::of(b.style.display) == Some(table::TablePart::Table)
            && let Some(c) = b.collapsed_border
        {
            padding = taffy::Rect::zero();
            border = taffy::Rect {
                top: c.top,
                right: c.right,
                bottom: c.bottom,
                left: c.left,
            };
        }
        let content_size = SizeF::new(
            (b.layout.size.width - padding.left - padding.right - border.left - border.right)
                .max(0.0),
            (b.layout.size.height - padding.top - padding.bottom - border.top - border.bottom)
                .max(0.0),
        );
        let floating = b.style.float != Float::None
            && b.parent
                .is_some_and(|parent| !self.boxes[parent].style.display.is_flexible_or_grid());
        let fixed_to_viewport = self.boxes[usize::from(id)].style.is_fixed()
            && !self.contains_fixed_above(usize::from(id));
        let sticky_below = children
            .iter()
            .any(|child| child.fragment.sticky_below || sticky::sticks(&child.fragment));
        let b = &mut self.boxes[usize::from(id)];
        let fragment = Arc::new(BoxFragment {
            node: b.node,
            pseudo: b.pseudo,
            role: b.role,
            floating,
            image: b
                .replaced
                .as_ref()
                .and_then(|r| r.image.as_ref().map(|(data, _)| data.clone())),
            canvas: b
                .replaced
                .as_ref()
                .is_some_and(replaced::Replaced::is_canvas),
            svg: b
                .replaced
                .as_ref()
                .and_then(|r| r.svg.as_ref())
                .map(|source| Arc::new(source.lay_out(content_size))),
            svg_image: b
                .replaced
                .as_ref()
                .and_then(|r| r.svg_image.map(|(image, _)| image)),
            object_rect: b.replaced.as_ref().filter(|r| r.svg.is_none()).map(|r| {
                let content = gfx::RectF::new(
                    border.left + padding.left,
                    border.top + padding.top,
                    content_size.width,
                    content_size.height,
                );
                r.object_rect(&b.style, content)
            }),
            columns: b.table_columns.clone(),
            hidden: b.hidden || b.clamped,
            fixed_to_viewport,
            sticky_below,
            clip_path: clip_path::box_clip_paths(
                &b.style,
                SizeF::new(b.layout.size.width, b.layout.size.height),
                [&sides(border), &sides(padding), &sides(b.layout.margin)],
                b.clip_source.as_deref(),
            ),
            border_collapsed: b.collapsed_border.is_some(),
            background_inside_border: b.background_inside_border,
            collapsed_borders: b.collapsed_borders.clone(),
            control_state: b.control_state,
            style: b.style.clone(),
            size: SizeF::new(b.layout.size.width, b.layout.size.height),
            padding: sides(padding),
            border: sides(border),
            margin: sides(b.layout.margin),
            children,
            out_of_flow,
            lines,
        });
        b.fragment = Some(fragment.clone());
        b.needs_fragment = false;
        b.needs_layout = false;
        self.last_layout.fragments = self.last_layout.fragments.wrapping_add(1);
        fragment
    }

    /// Box `index`'s out-of-flow children where they are in tree order,
    /// each before the first of `in_flow` (its children that come first in
    /// its fragment) after it. Each is laid out, hoisted to its containing
    /// block, every time it is (the box tree has no `display: none` boxes,
    /// which taffy's hidden layout would leave unhoisted). Their fragments
    /// as they are now: one made before may be made again for another
    /// out-of-flow box in it, made after it.
    fn out_of_flow_places(
        &mut self,
        index: usize,
        in_flow: &[taffy::NodeId],
    ) -> Vec<OutOfFlowPlace> {
        let mut places = Vec::new();
        let mut before = 0;
        for i in 0..self.layout_children(index).len() {
            let child = self.layout_children(index)[i];
            let b = &self.boxes[usize::from(child)];
            if !b.style.is_out_of_flow() {
                if in_flow.get(before) == Some(&child) {
                    #[expect(clippy::arithmetic_side_effects, reason = "in_flow[before] is there")]
                    {
                        before += 1;
                    }
                }
                continue;
            }
            places.push(OutOfFlowPlace {
                before,
                fragment: self.fragment(child),
            });
        }
        places
    }

    /// List item `index`'s outside marker, where it goes: its end at the
    /// start of the first line (moved by the floats beside it, not by
    /// `text-align`), its baseline on the line's; at the start and top of
    /// the content box without one (Blink's `LayoutOutsideListMarker`,
    /// placed by `ListMarker`).
    fn marker_link(&mut self, index: usize, marker: taffy::NodeId) -> FragmentLink {
        let layout = self.boxes[index].layout;
        let content = PointF::new(
            layout.border.left + layout.padding.left,
            layout.border.top + layout.padding.top,
        );
        let size = self.boxes[usize::from(marker)].layout.size;
        let at = match (self.first_line(index), self.first_line(marker.into())) {
            (Some(line), Some(own)) => PointF::new(line.x, line.y - own.y),
            _ => content,
        };
        FragmentLink {
            offset: PointF::new(at.x - size.width, at.y),
            fragment: self.fragment(marker),
        }
    }

    /// Where box `index`'s first line starts, and its baseline, from its
    /// border box's corner: its own, or the first of its in-flow children's
    /// that has one (not a float's, nor an out-of-flow box's).
    fn first_line(&self, index: usize) -> Option<PointF> {
        let b = &self.boxes[index];
        if let Some(inline) = &b.inline {
            let (border, padding) = (b.layout.border, b.layout.padding);
            return inline.first_line().map(|(start, baseline)| {
                PointF::new(
                    border.left + padding.left + start,
                    border.top + padding.top + b.content_shift + baseline,
                )
            });
        }
        if b.replaced.is_some() {
            return None;
        }
        self.layout_children(index).iter().find_map(|&id| {
            let child = &self.boxes[usize::from(id)];
            if child.style.is_out_of_flow() || child.style.float != Float::None {
                return None;
            }
            let location = child.layout.location;
            self.first_line(usize::from(id))
                .map(|line| PointF::new(location.x + line.x, location.y + line.y))
        })
    }

    /// Inline formatting context `index`'s lines, broken again at its
    /// final width (taffy may have measured other widths since), and its
    /// atomic inlines, laid out there, placed.
    fn inline_fragments(&mut self, index: usize) -> Vec<LineFragment> {
        let mut inline = self.boxes[index]
            .inline
            .take()
            .expect("an inline formatting context");
        let Layout {
            size,
            padding,
            border,
            ..
        } = self.boxes[index].layout;
        let shift = self.boxes[index].content_shift;
        let width = size.width - padding.left - padding.right - border.left - border.right;
        let beside = inline.laid_out_beside().clone();
        let broken = self.last_layout.lines_broken;
        inline.break_lines(Some(width.max(0.0)), &beside, self);
        if self.last_layout.lines_broken != broken {
            let update = &mut self.last_layout;
            update.lines_broken_for_fragments = update.lines_broken_for_fragments.wrapping_add(1);
        }
        let (lines, placed, text) = inline.fragments(PointF::new(
            border.left + padding.left,
            border.top + padding.top + shift,
        ));
        self.count_text_placed(text);
        for (&atomic, at) in inline.atomics().iter().zip(placed) {
            let b = &mut self.boxes[usize::from(atomic)];
            // Its own `position: relative` too, as taffy does for the
            // boxes it places.
            let by = relative_offset(&b.style, width.max(0.0), inline.height());
            b.layout.location = taffy::Point {
                x: at.x + b.layout.margin.left + by.x,
                y: at.y + b.layout.margin.top + by.y,
            };
        }
        self.boxes[index].inline = Some(inline);
        lines
    }

    /// Lays out inline formatting context `index`: a leaf to taffy, whose
    /// measure breaks the lines with parley. Its content is out of the box
    /// meanwhile, so the box tree can lay its atomic inlines out.
    fn compute_inline(
        &mut self,
        index: usize,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        let mut inline = self.boxes[index]
            .inline
            .take()
            .expect("an inline formatting context");
        let style = std::mem::take(&mut self.boxes[index].taffy_style);
        // taffy's leaf gives a measure function the sizes known of the
        // border box (measuring), and the content box's available space:
        // what it measures is the content box.
        let inset = content_box_inset(&style, inputs.parent_size.width);
        inline.set_height(definite_height(&style, &inputs, inset));
        let bands = block_ctx
            .as_deref()
            .map_or_else(FloatBands::default, |ctx| float_bands(ctx, inset));
        let mut output = compute_leaf_layout(
            inputs,
            &style,
            |_, _| 0.0,
            |known, available| {
                let known = content_box_size(known, inset);
                let floats = inline.floats().next().is_some();
                // Not given a width, it is as wide as the room, but no
                // narrower than its min-content width nor wider than its
                // max-content one (css-sizing 3, "fit-content size"), not
                // only as wide as its widest line there.
                let fit = match (known.width, available.width) {
                    (None, AvailableSpace::Definite(room)) => {
                        let (min, max) = inline.content_widths(self);
                        Some(room.min(max).max(min))
                    }
                    // Its floats beside its lines, which are only broken
                    // with them where they were.
                    (None, AvailableSpace::MaxContent) if floats => {
                        Some(inline.content_widths(self).1)
                    }
                    _ => None,
                };
                // Asked for its width alone, its content widths give it
                // without breaking a line (Blink's
                // `InlineNode::ComputeMinMaxSizes` builds no line boxes):
                // broken at its min-content width for a height nothing
                // reads, a paragraph was broken once more for each
                // shrink-to-fit or flex container measuring it. With floats
                // its width is where they were placed beside its lines.
                if inputs.run_mode == RunMode::ComputeSize
                    && inputs.axis == RequestedAxis::Horizontal
                    && !floats
                {
                    let width = known.width.or(fit).unwrap_or_else(|| {
                        let (min, max) = inline.content_widths(self);
                        match available.width {
                            AvailableSpace::MinContent => min,
                            _ => max,
                        }
                    });
                    // taffy reads only the width it asked for.
                    return taffy::Size { width, height: 0.0 };
                }
                // With floats, the lines are broken at the width it will
                // be: its floats are placed there. Without, the room breaks
                // them the same, and lays out the atomic inlines in the
                // space they end up in.
                let width = known
                    .width
                    .or(fit.filter(|_| floats))
                    .or(match available.width {
                        AvailableSpace::Definite(room) => Some(room),
                        AvailableSpace::MinContent => Some(inline.content_widths(self).0),
                        AvailableSpace::MaxContent => None,
                    });
                // Its floats go into the block formatting context it is in,
                // or one of its own that holds them (CSS 2 § 10.6.7).
                let mut size = match (block_ctx, width) {
                    (Some(ctx), Some(_)) if floats => {
                        let mut children = InlineChildren::new(self, ctx, inset);
                        inline.break_lines(width, &bands, &mut children)
                    }
                    (None, Some(width)) if floats => {
                        let mut root = BlockFormattingContext::new();
                        let mut ctx = root.root_block_context();
                        ctx.set_width(width + inset.left + inset.right);
                        let mut children = InlineChildren::new(self, &mut ctx, inset);
                        let mut size = inline.break_lines(Some(width), &bands, &mut children);
                        size.height = size.height.max(inline.floats_bottom());
                        size
                    }
                    _ => inline.break_lines(width, &bands, self),
                };
                size.width = known.width.or(fit).unwrap_or(size.width);
                taffy::Size {
                    width: size.width,
                    height: known.height.unwrap_or(size.height),
                }
            },
        );
        // Its first line's baseline, from the border box's top, for a flex
        // container's baseline alignment and to pass up as its own
        // (css-align 3, "first baseline set"). Laid out for real, taffy's
        // leaf has just measured it at its final width: the lines are the
        // ones it ends up with.
        if inputs.run_mode == RunMode::PerformLayout {
            // None with layout containment.
            output.baselines.first = inline
                .first_baseline()
                .filter(|_| {
                    !applied_containment(&self.boxes[index].style)
                        .contains(computed_style::Contain::LAYOUT)
                })
                .map(|baseline| inset.top + baseline);
            // Its padding box, which it lays out the out-of-flow boxes it
            // is the containing block of against.
            let border = style
                .border
                .resolve_or_zero(inputs.parent_size.width, |_, _| 0.0);
            output.oof_positioning_area = Some(OofPositioningArea {
                size: taffy::Size {
                    width: (output.size.width - border.left - border.right).max(0.0),
                    height: (output.size.height - border.top - border.bottom).max(0.0),
                },
                offset: taffy::Point {
                    x: border.left,
                    y: border.top,
                },
            });
        }
        // Its style back first: what it passes up is aligned by it.
        self.boxes[index].taffy_style = style;
        if inputs.run_mode == RunMode::PerformLayout {
            // Its fragments are made from these lines, beside these floats.
            inline.keep_laid_out_beside();
            self.place_floats(&inline, output.size.width, inset);
            output.oof_candidates = self.passed_up(index, &inline, &output, inset);
        }
        self.boxes[index].inline = Some(inline);
        output
    }

    /// Puts the floats of `inline`, laid out for real, where its lines
    /// placed them: from the border box, `width` wide, its content box
    /// `inset` into it.
    fn place_floats(&mut self, inline: &InlineContent, width: f32, inset: taffy::Rect<f32>) {
        let content_width = (width - inset.left - inset.right).max(0.0);
        for (float, at) in inline.floats() {
            let b = &mut self.boxes[usize::from(float)];
            // Its own `position: relative` too, as for an atomic inline.
            let by = relative_offset(&b.style, content_width, inline.height());
            b.layout.location = taffy::Point {
                x: inset.left + at.x + b.layout.margin.left + by.x,
                y: inset.top + at.y + b.layout.margin.top + by.y,
            };
        }
    }

    /// What inline formatting context `index`, laid out for real into
    /// `output`, passes up to the containing blocks of out-of-flow boxes, in
    /// tree order (Blink's `InlineLayoutAlgorithm::PlaceOutOfFlowObjects`):
    /// each of its own at its static position, and those its atomic
    /// inlines passed up, from where its lines put them. All from its
    /// border box; `inset` is its content box's.
    fn passed_up(
        &self,
        index: usize,
        inline: &InlineContent,
        output: &LayoutOutput,
        inset: taffy::Rect<f32>,
    ) -> OofCandidates {
        let mut candidates = OofCandidates::new();
        let from_atomics = inline
            .atomics()
            .iter()
            .copied()
            .chain(inline.floats().map(|(float, _)| float))
            .any(|atomic| !self.boxes[usize::from(atomic)].oof_candidates.is_empty());
        if inline.out_of_flow().next().is_none() && !from_atomics {
            return candidates;
        }
        let origin = PointF::new(inset.left, inset.top);
        let width = (output.size.width - inset.left - inset.right).max(0.0);
        let statics = inline.static_positions(origin, width);
        let placed = if from_atomics {
            inline.fragments(origin).1
        } else {
            Vec::new()
        };
        let own: Vec<taffy::NodeId> = inline.out_of_flow().collect();
        let justify_items = self.boxes[index].taffy_style.justify_items;
        for item in inline.passed_up() {
            match item {
                PassedUp::OutOfFlow(i) => {
                    let id = own[i];
                    // Aligned in the rectangle by its own `justify-self`
                    // (the container's `justify-items` for `auto`) and
                    // `align-self` (`normal` for `auto`), as taffy's block
                    // layout aligns a block's out-of-flow children.
                    let style = &self.boxes[usize::from(id)].taffy_style;
                    let ltr = taffy::Direction::Ltr;
                    let justify = style
                        .justify_self
                        .unwrap_or(justify_items)
                        .resolve_self_relative(ltr, ltr, true);
                    let align = style
                        .align_self
                        .unwrap_or(taffy::AlignItems::NORMAL)
                        .resolve_self_relative(ltr, ltr, false);
                    let rect = statics[i];
                    candidates.push(OofCandidate {
                        node: id,
                        order: self.boxes[index]
                            .children
                            .iter()
                            .position(|&child| child == id)
                            .map_or(0, base::numerics::saturated_cast),
                        position: self.boxes[usize::from(id)].taffy_style.position,
                        static_position: taffy::Point {
                            x: AxisStaticPosition::from_alignment(
                                justify,
                                taffy::Line {
                                    start: rect.x,
                                    end: rect.x + rect.width,
                                },
                                false,
                            ),
                            y: AxisStaticPosition::from_alignment(
                                align,
                                taffy::Line {
                                    start: rect.y,
                                    end: rect.y + rect.height,
                                },
                                false,
                            ),
                        },
                    });
                }
                PassedUp::Atomic(i) => {
                    let b = &self.boxes[usize::from(inline.atomics()[i])];
                    if b.oof_candidates.is_empty() {
                        continue;
                    }
                    // Where `inline_fragments` puts its border box.
                    let by = relative_offset(&b.style, width, inline.height());
                    let mut theirs = b.oof_candidates.clone();
                    theirs.translate(taffy::Point {
                        x: placed[i].x + b.layout.margin.left + by.x,
                        y: placed[i].y + b.layout.margin.top + by.y,
                    });
                    candidates.append(&mut theirs);
                }
                // From where `place_floats` put it.
                PassedUp::Float(i) => {
                    let (float, _) = inline.floats().nth(i).expect("one of its floats");
                    let b = &self.boxes[usize::from(float)];
                    let mut theirs = b.oof_candidates.clone();
                    theirs.translate(b.layout.location);
                    candidates.append(&mut theirs);
                }
            }
        }
        candidates
    }

    /// Atomic inline `index` takes `border_box` and `edge`'s padding,
    /// border and margin; its fragment is made again if they changed, or
    /// it is to be laid out again.
    fn set_atomic_layout(&mut self, index: usize, border_box: SizeF, edge: &Edge) {
        let rect = |s: Sides<f32>| taffy::Rect {
            top: s.top,
            right: s.right,
            bottom: s.bottom,
            left: s.left,
        };
        let b = &mut self.boxes[index];
        let layout = Layout {
            size: taffy::Size {
                width: border_box.width,
                height: border_box.height,
            },
            padding: rect(edge.padding),
            border: rect(edge.border),
            margin: rect(edge.margin),
            ..b.layout
        };
        if layout != b.layout || b.needs_layout {
            b.needs_fragment = true;
        }
        b.layout = layout;
    }

    /// The baseline of box `index`'s last line box, from its border box's
    /// top: its own, or its last child's that has one (CSS 2 § 10.8.1;
    /// Blink's `LayoutBlock::InlineBlockBaseline`). A child that clips its
    /// overflow has its bottom margin edge as its baseline there, as Blink
    /// synthesizes a scroll container's.
    fn last_baseline(&self, index: usize) -> Option<f32> {
        let b = &self.boxes[index];
        if let Some(inline) = &b.inline {
            let top = b.layout.border.top + b.layout.padding.top + b.content_shift;
            return inline.last_baseline().map(|baseline| top + baseline);
        }
        // A table's is its first row's (Blink's `LayoutTable::
        // InlineBlockBaseline`).
        if table::TablePart::of(b.style.display) == Some(table::TablePart::Table) {
            return b.table_baseline;
        }
        self.layout_children(index).iter().rev().find_map(|&id| {
            let child = &self.boxes[usize::from(id)];
            // Nor has one with layout containment a baseline to give.
            if child.style.is_out_of_flow()
                || applied_containment(&child.style).contains(computed_style::Contain::LAYOUT)
            {
                return None;
            }
            let baseline = if child.style.clips_overflow() {
                Some(child.layout.size.height + child.layout.margin.bottom)
            } else {
                self.last_baseline(usize::from(id))
            };
            baseline.map(|baseline| child.layout.location.y + baseline)
        })
    }

    /// taffy's `compute_child_layout` (as `TaffyTree` does it). Nested
    /// blocks share their parent's block formatting context through
    /// `block_ctx`; only floats use it, and they are off. Margin collapsing
    /// goes through `LayoutOutput` instead. Laid out for real, a box lays
    /// out the out-of-flow boxes passed up to it that it is the containing
    /// block of and passes the others on, inside the cache: a cached result
    /// has them done already.
    fn compute_box(
        &mut self,
        id: taffy::NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, id);
        }
        let index = usize::from(id);
        let mut block_ctx = block_ctx;
        // taffy's cache keys on `inputs` alone, not on the floats around:
        // a box next to floats, or that put floats of its own into the
        // block formatting context it shares, is laid out every time, and
        // its result not kept (Blink keys its cache on the exclusion space
        // instead).
        let floats = block_ctx
            .as_ref()
            .is_some_and(|ctx| self.boxes[index].places_floats || ctx.has_active_floats(0.0));
        let output = if floats {
            let update = &mut self.last_layout;
            update.laid_out_beside_floats = update.laid_out_beside_floats.wrapping_add(1);
            self.lay_out_box(id, inputs, block_ctx.as_deref_mut())
        } else {
            compute_cached_layout(self, id, inputs, |tree, id, inputs| {
                tree.lay_out_box(id, inputs, block_ctx.as_deref_mut())
            })
        };
        // A cached result put none: it was kept because it didn't.
        self.boxes[index].places_floats = block_ctx
            .is_some_and(|ctx| ctx.floated_content_height_contribution() > f32::NEG_INFINITY);
        output
    }

    /// Lays box `id` out, not from taffy's cache: its content, then, laid
    /// out for real, the out-of-flow boxes passed up to it that it is the
    /// containing block of.
    fn lay_out_box(
        &mut self,
        id: taffy::NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        let index = usize::from(id);
        // Size containment: what is in the box counts for nothing in its
        // size (Blink's `ShouldApplySizeContainment`, with no
        // `contain-intrinsic-size`). Not given a width and with none of
        // its own, it is as wide as its padding and border: so are its
        // intrinsic sizes and its shrink-to-fit width (a block in flow is
        // given its width).
        let contain = applied_containment(&self.boxes[index].style);
        // A grid's is what it would be without its items, its tracks as
        // they are (css-contain 2, "size containment"; Blink's `LayoutGrid`).
        let grid = self.boxes[index].taffy_style.display == taffy::Display::Grid;
        let mut inputs = inputs;
        if contain.contains(computed_style::Contain::INLINE_SIZE)
            && inputs.known_dimensions.width.is_none()
        {
            let style = &self.boxes[index].taffy_style;
            let parent = inputs.parent_size.width;
            let calc = |_, _| 0.0;
            if style.size.width.maybe_resolve(parent, calc).is_none() {
                inputs.known_dimensions.width = Some(if grid {
                    self.empty_grid_size(id, inputs).width
                } else {
                    let inset = content_box_inset(style, parent);
                    contained_size(
                        inset.left + inset.right,
                        style.box_sizing,
                        style.min_size.width.maybe_resolve(parent, calc),
                    )
                });
            }
        }
        // A line clamp container's height is where its clamped line ends,
        // which only laying it out tells: measured for its height, it is
        // laid out.
        if inputs.run_mode == RunMode::ComputeSize
            && inputs.axis != RequestedAxis::Horizontal
            && self.boxes[index].style.line_clamp().is_some()
        {
            let inputs = LayoutInput {
                run_mode: RunMode::PerformLayout,
                ..inputs
            };
            return self.lay_out_box(id, inputs, block_ctx);
        }
        // Laid out for real, not only measured: its fragment changes.
        if inputs.run_mode == RunMode::PerformLayout {
            self.boxes[index].needs_fragment = true;
            self.boxes[index].content_shift = 0.0;
            self.last_layout.laid_out = self.last_layout.laid_out.wrapping_add(1);
        }
        if matches!(
            self.boxes[index].taffy_style.display,
            taffy::Display::Flex | taffy::Display::Grid
        ) {
            self.order_children(index);
        }
        // A list item without lines is as tall as its marker at least, at
        // the top of its content box (Blink's `ListMarker::
        // AddToBoxWithoutLineBoxes`): one with no content at all here.
        let marker_height = match self.boxes[index].marker {
            Some(marker) => {
                self.lay_out(marker, AvailableSpace::MaxContent, None)
                    .margin_box
                    .height
            }
            None => 0.0,
        };
        let mut output = if table::TablePart::of(self.boxes[index].style.display)
            == Some(table::TablePart::Table)
        {
            self.compute_table(index, inputs)
        } else if self.boxes[index].inline.is_some() {
            self.compute_inline(index, inputs, block_ctx)
        } else {
            self.compute_content(id, inputs, block_ctx, marker_height)
        };
        if inputs.run_mode == RunMode::PerformLayout {
            // A flex or grid button lays its items out as the container
            // does. A text field's line is in the middle, where it
            // doesn't fit too.
            let b = &self.boxes[index];
            let centered = if b.button && !b.style.display.is_flexible_or_grid() {
                Some(true)
            } else {
                b.control
                    .as_ref()
                    .and_then(control::Control::centers_content)
            };
            if let Some(safe) = centered {
                let inset =
                    content_box_inset(&self.boxes[index].taffy_style, inputs.parent_size.width);
                self.center_content(index, inset, &output, safe);
            }
            self.place_slider_thumbs(index, inputs.parent_size.width, &output);
            if let Some(lines) = self.boxes[index].style.line_clamp() {
                self.clamp_lines(index, lines as usize, &inputs, &mut output);
            }
        }
        // Size containment: with no height given or of its own, it is as
        // tall as its padding and border, what is in it laid out (it
        // overflows).
        if contain.contains(computed_style::Contain::SIZE)
            && inputs.known_dimensions.height.is_none()
        {
            let style = &self.boxes[index].taffy_style;
            let parent = inputs.parent_size.height;
            let calc = |_, _| 0.0;
            if style.size.height.maybe_resolve(parent, calc).is_none() {
                let inset = content_box_inset(style, inputs.parent_size.width);
                let min = style.min_size.height.maybe_resolve(parent, calc);
                let box_sizing = style.box_sizing;
                output.size.height = if grid {
                    let known = taffy::Size {
                        width: Some(output.size.width),
                        height: None,
                    };
                    let inputs = LayoutInput {
                        known_dimensions: known,
                        ..inputs
                    };
                    self.empty_grid_size(id, inputs).height
                } else {
                    contained_size(inset.top + inset.bottom, box_sizing, min)
                };
            }
        }
        if inputs.run_mode == RunMode::PerformLayout {
            compute_oof_layout(self, id, &mut output);
        }
        output
    }

    /// Whether a box around box `index` is the containing block of a
    /// fixed box in it ([`contains_out_of_flow`]): the viewport isn't.
    fn contains_fixed_above(&self, index: usize) -> bool {
        let mut next = self.boxes[index].parent;
        while let Some(i) = next {
            let b = &self.boxes[i];
            if contains_out_of_flow(&b.style, b.role == BoxRole::RootElement) {
                return true;
            }
            next = self.boxes[i].parent;
        }
        false
    }

    /// A line clamp container's lines (`-webkit-line-clamp`; Blink's
    /// `LineClampData`), box `index` laid out to `output`: its block
    /// formatting context's lines in order, through its in-flow blocks but
    /// not into the formatting contexts they hold. If something comes after
    /// the `lines`th, that line ends with an ellipsis and what comes after
    /// it is hidden, laid out but neither painted nor hit; with no height of
    /// its own the box ends with that line, and its padding and border.
    /// Otherwise the clamp does nothing.
    fn clamp_lines(
        &mut self,
        index: usize,
        lines: usize,
        inputs: &LayoutInput,
        output: &mut LayoutOutput,
    ) {
        let style = &self.boxes[index].taffy_style;
        let inset = content_box_inset(style, inputs.parent_size.width);
        let mut find = ClampFind {
            remaining: lines,
            point: None,
            more: false,
        };
        self.find_clamp_point(index, 0.0, inset.top, &mut find);
        let point = find.point.filter(|_| find.more);
        let mut changed = Vec::new();
        self.set_clamp_point(
            index,
            point.map(|(clamped, line, _)| (clamped, line)),
            &mut false,
            &mut changed,
        );
        // The fragments of what changed, and those above it, are made
        // again.
        for changed in changed {
            let mut next = Some(changed);
            while let Some(i) = next {
                self.boxes[i].needs_fragment = true;
                next = self.boxes[i].parent.filter(|_| i != index);
            }
        }
        let Some((clamped, _, mut bottom)) = point else {
            return;
        };
        // The blocks around the clamped line end with it too, and their
        // block-end padding and border (Blink's line clamp sets their
        // block size at the clamp point).
        let mut chain = Vec::new();
        let mut next = Some(clamped).filter(|&i| i != index);
        while let Some(i) = next {
            chain.push(i);
            next = self.boxes[i].parent.filter(|&parent| parent != index);
        }
        let tops: Vec<f32> = (0..chain.len())
            .map(|k| {
                chain[k..]
                    .iter()
                    .map(|&i| self.boxes[i].layout.location.y)
                    .sum()
            })
            .collect();
        for (&i, top) in chain.iter().zip(tops) {
            let layout = &mut self.boxes[i].layout;
            bottom += layout.padding.bottom + layout.border.bottom;
            layout.size.height = bottom - top;
            self.boxes[i].needs_fragment = true;
        }
        let style = &self.boxes[index].taffy_style;
        let parent = inputs.parent_size.height;
        let calc = |_, _| 0.0;
        if inputs.known_dimensions.height.is_some()
            || style.size.height.maybe_resolve(parent, calc).is_some()
        {
            return;
        }
        let extra = if style.box_sizing == taffy::BoxSizing::ContentBox {
            inset.top + inset.bottom
        } else {
            0.0
        };
        let min = style.min_size.height.maybe_resolve(parent, calc);
        let max = style.max_size.height.maybe_resolve(parent, calc);
        let mut height = bottom + inset.bottom;
        if let Some(max) = max {
            height = height.min(max + extra);
        }
        if let Some(min) = min {
            height = height.max(min + extra);
        }
        output.size.height = height;
    }

    /// Where [`Self::clamp_lines`] clamps in box `index`, whose border box
    /// is `top` and content box `content_top` down the container's border
    /// box, and whether something comes after it.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "find.remaining starts at the clamp's count of lines, at least 1, and loses \
                  only fewer lines than it has: remaining - 1 < count"
    )]
    fn find_clamp_point(&self, index: usize, top: f32, content_top: f32, find: &mut ClampFind) {
        if find.more {
            return;
        }
        let b = &self.boxes[index];
        if let Some(inline) = &b.inline {
            let count = inline.line_count();
            if find.point.is_some() {
                find.more = count > 0;
            } else if find.remaining <= count {
                let line = find.remaining - 1;
                find.point = Some((index, line, content_top + inline.line_bottom(line)));
                find.more = line + 1 < count;
            } else {
                find.remaining -= count;
            }
            return;
        }
        for &child in &b.children {
            let c = &self.boxes[usize::from(child)];
            if in_same_block_formatting_context(c) {
                let top = top + c.layout.location.y;
                let content_top = top + c.layout.border.top + c.layout.padding.top;
                self.find_clamp_point(usize::from(child), top, content_top, find);
            } else if find.point.is_some() && !c.style.is_out_of_flow() {
                // Content of its own after the clamped line.
                find.more = true;
            }
            if find.more {
                return;
            }
        }
    }

    /// Ends box `index`'s lines at `point` (the box with the clamped line,
    /// and the line), hiding what comes after it once `past`; `None` hides
    /// nothing. The boxes that change go into `changed`.
    fn set_clamp_point(
        &mut self,
        index: usize,
        point: Option<(usize, usize)>,
        past: &mut bool,
        changed: &mut Vec<usize>,
    ) {
        if let Some(inline) = self.boxes[index].inline.as_mut() {
            let clamp = point.filter(|&(at, _)| at == index).map(|(_, line)| line);
            if inline.set_clamp(clamp) {
                changed.push(index);
            }
            *past |= clamp.is_some();
            return;
        }
        let children = self.boxes[index].children.clone();
        for child in children {
            let child = usize::from(child);
            let b = &mut self.boxes[child];
            if b.clamped != *past {
                b.clamped = *past;
                changed.push(child);
            }
            if !*past && in_same_block_formatting_context(b) {
                self.set_clamp_point(child, point, past, changed);
            }
        }
    }

    /// The tallest row of list box `index`'s, as last laid out: its
    /// options', and those in its optgroups with their labels.
    fn tallest_list_box_row(&self, index: usize) -> Option<f32> {
        let mut tallest = None::<f32>;
        let mut stack = self.boxes[index].children.clone();
        while let Some(child) = stack.pop() {
            let b = &self.boxes[usize::from(child)];
            if b.list_box_row {
                tallest =
                    Some(tallest.map_or(b.layout.size.height, |t| t.max(b.layout.size.height)));
            } else {
                stack.extend(b.children.iter().copied());
            }
        }
        tallest
    }

    /// A range's track's thumb at its ratio: where its value is, between
    /// the track's start and its end less the thumb's size (Blink's
    /// `LayoutSliderTrack::UpdateLayout`), the track `output` as laid out.
    fn place_slider_thumbs(&mut self, index: usize, width: Option<f32>, output: &LayoutOutput) {
        let inset = content_box_inset(&self.boxes[index].taffy_style, width);
        let room = output.size.width - inset.left - inset.right;
        let children = self.boxes[index].children.clone();
        for child in children {
            let b = &mut self.boxes[usize::from(child)];
            if let Some(ratio) = b.slider_ratio {
                let thumb = b.layout.size.width + b.layout.margin.left + b.layout.margin.right;
                b.layout.location.x += ratio * (room - thumb).max(0.0);
            }
        }
    }

    /// HTML's button layout, and a text field's: box `index`'s content
    /// (its lines, or its in-flow children) moved down to the middle of its
    /// content box, `output` as laid out, `inset` its border and padding;
    /// where it doesn't fit, at its top if `safe` (a button's, as
    /// Chrome's), else in the middle still (a text field's).
    fn center_content(
        &mut self,
        index: usize,
        inset: taffy::Rect<f32>,
        output: &LayoutOutput,
        safe: bool,
    ) {
        let b = &self.boxes[index];
        let top = inset.top;
        let room = output.size.height - inset.top - inset.bottom;
        let height = match &b.inline {
            Some(inline) => inline.lines_height().unwrap_or(0.0),
            None => b
                .children
                .iter()
                .map(|&id| &self.boxes[usize::from(id)])
                .filter(|child| !child.style.is_out_of_flow())
                .map(|child| {
                    child.layout.location.y + child.layout.size.height + child.layout.margin.bottom
                        - top
                })
                .fold(0.0, f32::max),
        };
        let shift = (room - height) / 2.0;
        self.shift_content(index, if safe { shift.max(0.0) } else { shift });
    }

    /// Box `id`'s own content: block, flex or grid, as its `display` says
    /// (what taffy's algorithms do with its children).
    fn compute_content(
        &mut self,
        id: taffy::NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
        marker_height: f32,
    ) -> LayoutOutput {
        let index = usize::from(id);
        // A list box `size` rows high (where nothing else sizes it): its
        // rows laid out first, as tall as the tallest of them; within its
        // `min-height` and `max-height`.
        let calc = |_, _| 0.0;
        if let Some((rows, empty_row)) = self.boxes[index]
            .control
            .as_ref()
            .and_then(control::Control::list_box)
            && inputs.known_dimensions.height.is_none()
            && self.boxes[index]
                .taffy_style
                .size
                .height
                .maybe_resolve(inputs.parent_size.height, calc)
                .is_none()
        {
            let style = &self.boxes[index].taffy_style;
            let inset = content_box_inset(style, inputs.parent_size.width);
            let extra = if style.box_sizing == taffy::BoxSizing::ContentBox {
                inset.top + inset.bottom
            } else {
                0.0
            };
            let parent = inputs.parent_size.height;
            let min = style
                .min_size
                .height
                .maybe_resolve(parent, calc)
                .map(|h| h + extra);
            let max = style
                .max_size
                .height
                .maybe_resolve(parent, calc)
                .map(|h| h + extra);
            let measured = compute_block_layout(
                self,
                id,
                LayoutInput {
                    run_mode: RunMode::PerformLayout,
                    ..inputs
                },
                None,
            );
            let row = self.tallest_list_box_row(index).unwrap_or(empty_row);
            let sized = LayoutInput {
                known_dimensions: taffy::Size {
                    width: inputs.known_dimensions.width.or(Some(measured.size.width)),
                    height: Some(
                        (rows as f32 * row + inset.top + inset.bottom).maybe_clamp(min, max),
                    ),
                },
                ..inputs
            };
            return compute_block_layout(self, id, sized, block_ctx);
        }
        // A text control's inner editor: a line high where nothing else
        // sizes it, even with no text (Blink's `HasLineIfEmpty`), but where
        // its placeholder is shown, which is in that line's place.
        if let Some(line) = self.boxes[index].empty_line
            && !self.boxes[index].parent.is_some_and(|parent| {
                self.boxes[parent]
                    .children
                    .iter()
                    .any(|&child| self.boxes[usize::from(child)].placeholder)
            })
            && inputs.known_dimensions.height.is_none()
            && self.boxes[index]
                .taffy_style
                .size
                .height
                .maybe_resolve(inputs.parent_size.height, calc)
                .is_none()
        {
            let measured = compute_block_layout(
                self,
                id,
                LayoutInput {
                    run_mode: RunMode::PerformLayout,
                    ..inputs
                },
                None,
            );
            let inset = content_box_inset(&self.boxes[index].taffy_style, inputs.parent_size.width);
            if measured.size.height - inset.top - inset.bottom < line {
                let sized = LayoutInput {
                    known_dimensions: taffy::Size {
                        width: inputs.known_dimensions.width.or(Some(measured.size.width)),
                        height: Some(line + inset.top + inset.bottom),
                    },
                    ..inputs
                };
                return compute_block_layout(self, id, sized, block_ctx);
            }
            return compute_block_layout(self, id, inputs, block_ctx);
        }
        let LayoutBox {
            taffy_style,
            children,
            replaced,
            control,
            style,
            ..
        } = &mut self.boxes[index];
        let inset = content_box_inset(taffy_style, inputs.parent_size.width);
        if let Some(control) = control.as_ref()
            && let Some(sizing) = &control.sizing
        {
            // As a replaced element is, its own content size where its
            // style gives none; its baseline its own. Its content (a text
            // control's shadow tree's) is laid out in that size, not
            // sizing it.
            let known = inputs.known_dimensions;
            let mut output = compute_leaf_layout(
                inputs,
                &*taffy_style,
                |_, _| 0.0,
                |_, _| {
                    let known = content_box_size(known, inset);
                    let size = sizing.measure(known.width, known.height);
                    taffy::Size {
                        width: size.width,
                        height: size.height,
                    }
                },
            );
            let edge = Edge::of(style, inputs.parent_size.width.unwrap_or(0.0));
            output.baselines.first =
                control.baseline(SizeF::new(output.size.width, output.size.height), &edge);
            if !children.is_empty() && inputs.run_mode == RunMode::PerformLayout {
                let size = output.size;
                let sized = LayoutInput {
                    known_dimensions: taffy::Size {
                        width: Some(size.width),
                        height: Some(size.height),
                    },
                    ..inputs
                };
                let laid_out = compute_block_layout(self, id, sized, block_ctx);
                output.scrollable_overflow_rect = laid_out.scrollable_overflow_rect;
            }
            output
        } else if let Some(replaced) = replaced {
            // What its style doesn't size, its image does, from the
            // sizes the layout knows: taffy's leaf passes them to the
            // measure only when measuring, but a grid item stretched
            // on one axis is laid out with only that one known, and
            // its other follows through the ratio.
            // A percentage width or `max-width` makes it compressible: in
            // a min-content size its content takes no width, whatever
            // width it has otherwise (css-sizing 3, "compressible replaced
            // elements"; a `width: 100%` or `max-width: 100%` image in a
            // `1fr` grid column or a flex item shrinks with it, as in
            // Chrome), where its width isn't known yet. Its `min-width`
            // still applies.
            let compressible = inputs.available_space.width == AvailableSpace::MinContent
                && inputs.known_dimensions.width.is_none()
                && (matches!(style.width, Size::LengthPercentage(v) if v.has_percent())
                    || matches!(
                        style.max_width,
                        LengthPercentageOrNone::LengthPercentage(v) if v.has_percent()
                    ));
            let mut inputs = inputs;
            if compressible {
                inputs.known_dimensions.width = Some(inset.left + inset.right);
            }
            let known = inputs.known_dimensions;
            compute_leaf_layout(
                inputs,
                &*taffy_style,
                |_, _| 0.0,
                |_, _| {
                    let known = content_box_size(known, inset);
                    let size = replaced.measure(known.width, known.height);
                    taffy::Size {
                        width: size.width,
                        height: size.height,
                    }
                },
            )
        } else if children.is_empty() {
            // An empty block: nothing inside to size it but its marker.
            compute_leaf_layout(
                inputs,
                &*taffy_style,
                |_, _| 0.0,
                |_, _| taffy::Size {
                    width: 0.0,
                    height: marker_height,
                },
            )
        } else if taffy_style.display == taffy::Display::Flex {
            match self.column_flex_width(id, inputs) {
                Some(output) => output,
                None => match self.fit_content_width(id, inputs) {
                    Some(output) => output,
                    None => compute_flexbox_layout(self, id, inputs),
                },
            }
        } else if taffy_style.display == taffy::Display::Grid {
            match self.fit_content_width(id, inputs) {
                Some(output) => output,
                None => compute_grid_layout(self, id, inputs),
            }
        } else {
            let measuring =
                inputs.run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal;
            let outer = std::mem::replace(&mut self.measuring_width, measuring);
            let output = compute_block_layout(self, id, inputs, block_ctx);
            self.measuring_width = outer;
            output
        }
    }

    /// A single-line column flexbox's width alone, when it isn't known:
    /// the widest of its in-flow children's contributions with their
    /// margins, plus its own padding, border and scrollbar gutter, clamped
    /// by its `min-width` and `max-width` (Blink's
    /// `FlexLayoutAlgorithm::ComputeMinMaxSizes`, its fast path for a
    /// single-line column). taffy runs the flex algorithm through the
    /// children's flex base sizes for it, laying each child out at that
    /// width for its height, which the width doesn't need: a column of
    /// paragraphs had each broken into lines once more at its min-content
    /// width. Without the height each child flexes to, which only a ratio
    /// carries into a width: with a child that has one, or its own, or
    /// lines that wrap, `None`, and taffy sizes it.
    fn column_flex_width(
        &mut self,
        id: taffy::NodeId,
        inputs: LayoutInput,
    ) -> Option<LayoutOutput> {
        let index = usize::from(id);
        let style = &self.boxes[index].taffy_style;
        if inputs.run_mode != RunMode::ComputeSize
            || inputs.axis != RequestedAxis::Horizontal
            || inputs.known_dimensions.width.is_some()
            || !matches!(
                style.flex_direction,
                taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse
            )
            || style.flex_wrap != taffy::FlexWrap::NoWrap
            || style.aspect_ratio.is_some()
        {
            return None;
        }
        if self.layout_children(index).iter().any(|&child| {
            let s = &self.boxes[usize::from(child)].taffy_style;
            !s.position.is_out_of_flow()
                && s.display != taffy::Display::None
                && s.aspect_ratio.is_some()
        }) {
            return None;
        }
        let parent = inputs.parent_size.width;
        let calc = |handle, basis| self.resolve_calc_value(handle, basis);
        // A width its style gives, or a `max-width` no more than its
        // `min-width`: taffy takes it without laying anything out.
        let min = style.min_size.width.maybe_resolve(parent, calc);
        let max = style.max_size.width.maybe_resolve(parent, calc);
        if (inputs.sizing_mode == SizingMode::InherentSize
            && style.size.width.maybe_resolve(parent, calc).is_some())
            || matches!((min, max), (Some(min), Some(max)) if max <= min)
        {
            return None;
        }
        let padding = style.padding.resolve_or_zero(parent, calc);
        let border = style.border.resolve_or_zero(parent, calc);
        let edges = padding.left + padding.right + border.left + border.right;
        let edges_y = padding.top + padding.bottom + border.top + border.bottom;
        // A scrollbar that scrolls one way takes room the other way.
        let gutter = |overflow: taffy::Overflow| {
            if overflow == taffy::Overflow::Scroll {
                style.scrollbar_width
            } else {
                0.0
            }
        };
        let inset = edges + gutter(style.overflow.y);
        let inset_y = edges_y + gutter(style.overflow.x);
        let adjust = if style.box_sizing == taffy::BoxSizing::ContentBox {
            edges
        } else {
            0.0
        };
        let (min, max) = (min.maybe_add(adjust), max.maybe_add(adjust));
        let inner_height = inputs.known_dimensions.height.maybe_sub(inset_y);
        let available = taffy::Size {
            width: inputs.available_space.width.maybe_sub(inset).maybe_max(0.0),
            height: inner_height.map_or(
                inputs
                    .available_space
                    .height
                    .maybe_sub(inset_y)
                    .maybe_max(0.0),
                |height| AvailableSpace::Definite(height.max(0.0)),
            ),
        };
        let widest = |tree: &mut Self, width| {
            tree.widest_column_item(index, width, available.height, inner_height)
        };
        let widest = match available.width {
            // Shrink-to-fit, between its min-content and max-content
            // widths, as Blink sizes it from its min and max sizes: measured
            // in the room instead, a child that doesn't shrink with it (a
            // percentage-width image, which a min-content width compresses)
            // would count at its own width.
            AvailableSpace::Definite(room) => {
                let min = widest(self, AvailableSpace::MinContent);
                let max = widest(self, AvailableSpace::MaxContent);
                room.min(max).max(min)
            }
            width => widest(self, width),
        };
        let width = (widest + inset).maybe_clamp(min, max).max(edges);
        // A debug build checks it against taffy's own, without counting
        // what taffy's lays out again.
        #[cfg(debug_assertions)]
        {
            let counts = self.last_layout;
            let taffy = compute_flexbox_layout(self, id, inputs).size.width;
            self.last_layout = counts;
            assert!(
                (taffy - width).abs() <= 1.0 / 64.0,
                "column flexbox {index} ({:?}) is {width} px wide in {:?}, taffy says {taffy} px",
                self.boxes[index].node,
                inputs.available_space.width,
            );
        }
        Some(LayoutOutput::from_outer_size(taffy::Size {
            width,
            height: 0.0,
        }))
    }

    /// A flex or grid container's width, when it isn't known, in a
    /// definite room: shrink-to-fit, its max-content width but no wider
    /// than the room and no narrower than its min-content width, as Blink
    /// sizes any box from its min and max sizes; laid out, or measured for
    /// its height, at that width. taffy's algorithms don't shrink them
    /// with the room: a row flexbox takes the sum of its items' flex base
    /// sizes, a grid's `fr` tracks size to their items' max-content
    /// contributions, and a column's lines that wrap take all of it, so a
    /// float or an absolutely positioned box that is one, or that holds
    /// one, would be as wide as its text unbroken. `None` when its width
    /// is otherwise known, and taffy sizes it.
    fn fit_content_width(
        &mut self,
        id: taffy::NodeId,
        inputs: LayoutInput,
    ) -> Option<LayoutOutput> {
        let index = usize::from(id);
        let style = &self.boxes[index].taffy_style;
        let AvailableSpace::Definite(room) = inputs.available_space.width else {
            return None;
        };
        if inputs.known_dimensions.width.is_some() {
            return None;
        }
        let parent = inputs.parent_size.width;
        let calc = |handle, basis| self.resolve_calc_value(handle, basis);
        if inputs.sizing_mode == SizingMode::InherentSize
            && style.size.width.maybe_resolve(parent, calc).is_some()
        {
            return None;
        }
        let grid = style.display == taffy::Display::Grid;
        // Each from taffy's cache, if measured already.
        let width = |tree: &mut Self, width| {
            let inputs = LayoutInput {
                run_mode: RunMode::ComputeSize,
                axis: RequestedAxis::Horizontal,
                available_space: taffy::Size {
                    width,
                    ..inputs.available_space
                },
                ..inputs
            };
            tree.compute_child_layout(id, inputs).size.width
        };
        let min = width(self, AvailableSpace::MinContent);
        let max = width(self, AvailableSpace::MaxContent);
        let width = room.min(max).max(min);
        if inputs.run_mode == RunMode::ComputeSize && inputs.axis == RequestedAxis::Horizontal {
            return Some(LayoutOutput::from_outer_size(taffy::Size {
                width,
                height: 0.0,
            }));
        }
        let inputs = LayoutInput {
            known_dimensions: taffy::Size {
                width: Some(width),
                ..inputs.known_dimensions
            },
            ..inputs
        };
        Some(if grid {
            compute_grid_layout(self, id, inputs)
        } else {
            compute_flexbox_layout(self, id, inputs)
        })
    }

    /// The widest of column flexbox `index`'s in-flow children's
    /// min-content or max-content contributions (`width`), with their
    /// margins: each measured as taffy measures an item's hypothetical
    /// cross size, without the height it flexes to. `height` is the room
    /// below, `inner_height` the content box's height if known.
    fn widest_column_item(
        &mut self,
        index: usize,
        width: AvailableSpace,
        height: AvailableSpace,
        inner_height: Option<f32>,
    ) -> f32 {
        let mut widest = 0.0_f32;
        for i in 0..self.layout_children(index).len() {
            let child = self.layout_children(index)[i];
            let s = &self.boxes[usize::from(child)].taffy_style;
            if s.position.is_out_of_flow() || s.display == taffy::Display::None {
                continue;
            }
            // As taffy resolves an item's, against the content box's
            // width, which isn't known.
            let calc = |handle, basis| self.resolve_calc_value(handle, basis);
            let padding = s.padding.resolve_or_zero(None, calc);
            let border = s.border.resolve_or_zero(None, calc);
            let edges = padding.left + padding.right + border.left + border.right;
            let adjust = if s.box_sizing == taffy::BoxSizing::ContentBox {
                edges
            } else {
                0.0
            };
            let size = s.size.width.maybe_resolve(None, calc).maybe_add(adjust);
            let min = s.min_size.width.maybe_resolve(None, calc).maybe_add(adjust);
            let max = s.max_size.width.maybe_resolve(None, calc).maybe_add(adjust);
            let margin = s.margin.resolve_or_zero(None, calc);
            let (margin, margin_y) = (margin.left + margin.right, margin.top + margin.bottom);
            // A sizing keyword is measured under its own constraint; with
            // no content box width, only these resolve.
            let raw = s.size.width.into_raw();
            let room = match raw.tag() {
                taffy::CompactLength::MIN_CONTENT_TAG => AvailableSpace::MinContent,
                taffy::CompactLength::MAX_CONTENT_TAG => AvailableSpace::MaxContent,
                taffy::CompactLength::FIT_CONTENT_PX_TAG => AvailableSpace::Definite(raw.value()),
                _ => width,
            };
            let size = size.maybe_clamp(min, max).maybe_max(edges);
            let size = size.unwrap_or_else(|| {
                let measured = self.compute_child_layout(
                    child,
                    LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        sizing_mode: SizingMode::ContentSize,
                        axis: RequestedAxis::Horizontal,
                        known_dimensions: taffy::Size::NONE,
                        known_dimensions_are_definite: taffy::Size {
                            width: true,
                            height: true,
                        },
                        parent_size: taffy::Size {
                            width: None,
                            height: inner_height,
                        },
                        available_space: taffy::Size {
                            width: room,
                            height: height.map_definite_value(|room| (room - margin_y).max(0.0)),
                        },
                        vertical_margins_are_collapsible: taffy::Line::FALSE,
                    },
                );
                measured.size.width.maybe_clamp(min, max).max(edges)
            });
            widest = widest.max(size + margin);
        }
        widest
    }

    /// Box `index`'s children in the order layout takes them: a flex
    /// container's in `order`-modified document order. The out-of-flow
    /// ones among them only get their static positions there.
    fn layout_children(&self, index: usize) -> &[taffy::NodeId] {
        let b = &self.boxes[index];
        b.ordered.as_deref().unwrap_or(&b.children)
    }

    /// Sorts flex container `index`'s children by `order`, keeping document
    /// order among equal ones, if any isn't 0: each time it is laid out, as
    /// Blink's `FlexChildIterator` does, so an item's new `order` needs no
    /// more than laying the container out again, as any other change does.
    fn order_children(&mut self, index: usize) {
        // A `-webkit-box`'s by `-webkit-box-ordinal-group`, 1 at first.
        let webkit_box = self.boxes[index].style.display.is_webkit_box();
        let order = |child: &taffy::NodeId| {
            let style = &self.boxes[usize::from(*child)].style;
            if webkit_box {
                // From a positive integer: an i32 exactly.
                base::numerics::saturated_cast::<i32, _>(style.rare.box_ordinal_group)
                    .saturating_sub(1)
            } else {
                style.order
            }
        };
        let children = &self.boxes[index].children;
        let ordered = children.iter().any(|child| order(child) != 0).then(|| {
            let mut ordered = children.clone();
            // A stable sort.
            ordered.sort_by_key(order);
            ordered
        });
        self.boxes[index].ordered = ordered;
    }
}

/// The room the floats of the block formatting context `ctx` leave beside
/// the content box of the box it is for, `inset` into its border box
/// (Blink's layout opportunities): taffy's content slots, from the content
/// box's top down, one per segment of its float context, then the whole
/// width again below the lowest float. None if no float is beside it.
/// The slots are of the box's border box; a line's room is kept to the
/// content box when it is worked out from them.
fn float_bands(ctx: &BlockContext<'_>, inset: taffy::Rect<f32>) -> FloatBands {
    let top = inset.top;
    if !ctx.has_active_floats(top) {
        return FloatBands::default();
    }
    let band = |slot: taffy::ContentSlot| FloatBand {
        top: (slot.y - top).max(0.0),
        x: slot.x - inset.left,
        width: slot.width.max(0.0),
    };
    let mut bands = Vec::new();
    let mut after = None;
    loop {
        let slot = ctx.find_content_slot(top, taffy::Clear::None, after);
        let Some(segment) = slot.segment_id else {
            break;
        };
        bands.push(band(slot));
        after = Some(segment);
    }
    // A slot below every segment starts where it is asked for: the lowest
    // float's bottom.
    let bottom = ctx
        .cleared_threshold(taffy::Clear::Both)
        .unwrap_or(top)
        .max(top);
    bands.push(band(ctx.find_content_slot(
        bottom,
        taffy::Clear::None,
        after,
    )));
    FloatBands(bands)
}

/// The content box's height of a box laid out with `inputs`, if definite:
/// the height it is given, or its style's, between its minimum and maximum.
fn definite_height(
    style: &taffy::Style,
    inputs: &LayoutInput,
    inset: taffy::Rect<f32>,
) -> Option<f32> {
    // Sized by its content (a table cell's first layout), its own sizes
    // don't count.
    if inputs.sizing_mode == SizingMode::ContentSize {
        return inputs
            .known_dimensions
            .height
            .map(|height| (height - inset.top - inset.bottom).max(0.0));
    }
    let parent = inputs.parent_size.height;
    let content_box = style.box_sizing == taffy::BoxSizing::ContentBox;
    let extra = if content_box {
        inset.top + inset.bottom
    } else {
        0.0
    };
    let calc = |_, _| 0.0;
    let size = style.size.height.maybe_resolve(parent, calc);
    let min = style.min_size.height.maybe_resolve(parent, calc);
    let max = style.max_size.height.maybe_resolve(parent, calc);
    inputs
        .known_dimensions
        .height
        .or_else(|| {
            size.map(|h| h + extra)
                .maybe_clamp(min.map(|h| h + extra), max.map(|h| h + extra))
        })
        .map(|height| (height - inset.top - inset.bottom).max(0.0))
}

/// The content box's sizes known of `border_box`'s, inside `inset`.
fn content_box_size(
    border_box: taffy::Size<Option<f32>>,
    inset: taffy::Rect<f32>,
) -> taffy::Size<Option<f32>> {
    taffy::Size {
        width: border_box
            .width
            .map(|width| (width - inset.left - inset.right).max(0.0)),
        height: border_box
            .height
            .map(|height| (height - inset.top - inset.bottom).max(0.0)),
    }
}

/// A box's padding and border, which taffy resolves (percentages too)
/// against its parent's width.
fn content_box_inset(style: &taffy::Style, parent_width: Option<f32>) -> taffy::Rect<f32> {
    let padding = style.padding.resolve_or_zero(parent_width, |_, _| 0.0);
    let border = style.border.resolve_or_zero(parent_width, |_, _| 0.0);
    taffy::Rect {
        left: padding.left + border.left,
        right: padding.right + border.right,
        top: padding.top + border.top,
        bottom: padding.bottom + border.bottom,
    }
}

/// The containment of `contain` that applies to a box in `style` (Blink's
/// `IsEligibleForPaintOrLayoutContainment` and
/// `IsEligibleForSizeContainment`): none to a table row, row group or
/// column, no size containment to a table or a cell. (A non-atomic inline
/// box, which none applies to either, has no box of its own.)
pub(crate) fn applied_containment(style: &ComputedStyle) -> computed_style::Contain {
    use computed_style::Contain;
    let contain = style.rare.contain;
    match style.display {
        Display::Table(
            TableDisplay::Row
            | TableDisplay::RowGroup
            | TableDisplay::HeaderGroup
            | TableDisplay::FooterGroup
            | TableDisplay::Column
            | TableDisplay::ColumnGroup,
        ) => Contain::NONE,
        Display::Table(TableDisplay::Table | TableDisplay::Cell) | Display::InlineTable => {
            let mut without_size = Contain::NONE;
            for kind in [Contain::LAYOUT, Contain::STYLE, Contain::PAINT] {
                if contain.contains(kind) {
                    without_size = without_size.union(kind);
                }
            }
            without_size
        }
        _ => contain,
    }
}

/// Whether any containment applies to the box of an element in `style`
/// (Blink's `ShouldApplyAnyContainment` for the root element and
/// `<body>`): style containment, or one its box takes, an inline box none.
/// It keeps `<body>`'s background and `overflow` from the canvas and the
/// viewport.
pub fn applies_any_containment(style: &ComputedStyle) -> bool {
    style.rare.contain.contains(computed_style::Contain::STYLE)
        || (style.display != Display::Inline
            && applied_containment(style) != computed_style::Contain::NONE)
}

/// Whether a box in `style` clips what is in it to its padding box: its
/// `overflow` does, or paint containment (which clips both axes, without
/// being a scroll container; Blink's `ComputeOverflowClipAxes`).
pub fn clips_content(style: &ComputedStyle) -> bool {
    style.clips_overflow() || paint_contained(style)
}

/// Whether paint containment applies to a box in `style`.
pub fn paint_contained(style: &ComputedStyle) -> bool {
    applied_containment(style).contains(computed_style::Contain::PAINT)
}

/// Whether a box in `style` is the containing block of the absolute and
/// fixed boxes inside it whatever its `position`: it has a `transform`, a
/// `filter` (unless it is the `root`'s), or layout or paint containment
/// (Blink's `ComputeIsFixedContainer`, and `ComputeIsAbsoluteContainer`
/// for a fixed container).
pub(crate) fn contains_out_of_flow(style: &ComputedStyle, root: bool) -> bool {
    use computed_style::Contain;
    let contain = applied_containment(style);
    !style.transform.is_empty()
        || (!style.rare.filter.is_empty() && !root)
        || contain.contains(Contain::LAYOUT)
        || contain.contains(Contain::PAINT)
}

/// What taffy reads of a box as a container: its taffy style, and which
/// out-of-flow boxes inside it it is the containing block of, which taffy
/// leaves to this (`CoreStyle::is_containing_block`: by default `position`
/// alone, and fixed boxes never).
#[derive(Clone, Copy)]
pub struct ContainerStyle<'a> {
    style: &'a taffy::Style,
    /// [`contains_out_of_flow`].
    contains_out_of_flow: bool,
}

impl taffy::CoreStyle for ContainerStyle<'_> {
    type CustomIdent = String;

    fn box_generation_mode(&self) -> taffy::BoxGenerationMode {
        self.style.box_generation_mode()
    }
    fn is_block(&self) -> bool {
        self.style.is_block()
    }
    fn is_compressible_replaced(&self) -> bool {
        self.style.is_compressible_replaced()
    }
    fn is_replaced(&self) -> bool {
        self.style.is_replaced()
    }
    fn box_sizing(&self) -> taffy::BoxSizing {
        self.style.box_sizing()
    }
    fn direction(&self) -> taffy::Direction {
        self.style.direction()
    }
    fn overflow(&self) -> taffy::Point<taffy::Overflow> {
        self.style.overflow()
    }
    fn scrollbar_width(&self) -> f32 {
        self.style.scrollbar_width()
    }
    fn position(&self) -> taffy::Position {
        self.style.position()
    }
    fn inset(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        self.style.inset()
    }
    fn is_containing_block(&self) -> taffy::ContainingBlockClaims {
        taffy::ContainingBlockClaims {
            absolute: self.style.position().is_positioned() || self.contains_out_of_flow,
            fixed: self.contains_out_of_flow,
        }
    }
    fn size(&self) -> taffy::Size<taffy::Dimension> {
        self.style.size()
    }
    fn min_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        self.style.min_size()
    }
    fn max_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        self.style.max_size()
    }
    fn aspect_ratio(&self) -> Option<f32> {
        self.style.aspect_ratio()
    }
    fn margin(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        self.style.margin()
    }
    fn padding(&self) -> taffy::Rect<taffy::LengthPercentage> {
        self.style.padding()
    }
    fn border(&self) -> taffy::Rect<taffy::LengthPercentage> {
        self.style.border()
    }
    fn contain(&self) -> taffy::Contain {
        self.style.contain()
    }
}

/// A size-contained box's border-box size on an axis: its padding and
/// border `inset`, or `min` (of its box-sizing's box) if more. (A `max`
/// can't take it under its padding and border, which it already is.)
fn contained_size(inset: f32, box_sizing: taffy::BoxSizing, min: Option<f32>) -> f32 {
    let extra = if box_sizing == taffy::BoxSizing::ContentBox {
        inset
    } else {
        0.0
    };
    min.map_or(inset, |min| inset.max(min + extra))
}

/// What [`BoxTree::find_clamp_point`] has found.
struct ClampFind {
    /// The lines to go to the clamped one, which is the next if 1.
    remaining: usize,
    /// The box with the clamped line, the line, and its bottom down the
    /// container's border box, once reached.
    point: Option<(usize, usize, f32)>,
    /// Whether something comes after it.
    more: bool,
}

/// Whether box `b` is a block in the block formatting context it is in,
/// not one establishing its own (taffy's `is_in_same_bfc`): a block
/// container in flow, not floated, not a scroll container, nor a button
/// (`flow-root` isn't `block`).
fn in_same_block_formatting_context(b: &LayoutBox) -> bool {
    let style = &b.style;
    matches!(style.display, Display::Block | Display::ListItem)
        && !style.is_out_of_flow()
        && style.float == Float::None
        && !style.is_scroll_container()
        && b.replaced.is_none()
        && b.control.is_none()
        && !b.button
}

/// One entry of a track list, for taffy.
fn grid_track(track: &GridTrack, calcs: &CalcValues) -> taffy::GridTemplateComponent<String> {
    match track {
        GridTrack::Size(size) => taffy::GridTemplateComponent::Single(track_size(size, calcs)),
        GridTrack::Repeat(count, sizes, names) => {
            taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
                count: match *count {
                    RepeatCount::Count(n) => taffy::RepetitionCount::Count(n),
                    RepeatCount::AutoFill => taffy::RepetitionCount::AutoFill,
                    RepeatCount::AutoFit => taffy::RepetitionCount::AutoFit,
                },
                tracks: sizes.iter().map(|size| track_size(size, calcs)).collect(),
                line_names: line_names(names),
            })
        }
    }
}

/// A `<track-size>` as taffy's minimum and maximum sizing functions: a
/// breadth alone is both, but `<flex>`, whose minimum is `auto` (css-grid
/// 2, "grid-template-columns"); `fit-content()` is `auto` up to its
/// argument.
fn track_size(size: &TrackSize, calcs: &CalcValues) -> taffy::TrackSizingFunction {
    use taffy::{MaxTrackSizingFunction as Max, MinTrackSizingFunction as Min};

    let min = |b: TrackBreadth| match b {
        TrackBreadth::LengthPercentage(LengthPercentage::Px(px)) => Min::length(px),
        TrackBreadth::LengthPercentage(LengthPercentage::Percent(p)) => Min::percent(p / 100.0),
        TrackBreadth::LengthPercentage(LengthPercentage::Calc {
            px,
            percent,
            non_negative,
        }) => Min::calc(calcs.handle(px, percent, non_negative)),
        // Not a minimum.
        TrackBreadth::Flex(_) | TrackBreadth::Auto => Min::auto(),
        TrackBreadth::MinContent => Min::min_content(),
        TrackBreadth::MaxContent => Min::max_content(),
    };
    let max = |b: TrackBreadth| match b {
        TrackBreadth::LengthPercentage(LengthPercentage::Px(px)) => Max::length(px),
        TrackBreadth::LengthPercentage(LengthPercentage::Percent(p)) => Max::percent(p / 100.0),
        TrackBreadth::LengthPercentage(LengthPercentage::Calc {
            px,
            percent,
            non_negative,
        }) => Max::calc(calcs.handle(px, percent, non_negative)),
        TrackBreadth::Flex(fr) => Max::fr(fr),
        TrackBreadth::Auto => Max::auto(),
        TrackBreadth::MinContent => Max::min_content(),
        TrackBreadth::MaxContent => Max::max_content(),
    };
    match *size {
        TrackSize::Breadth(b) => taffy::MinMax {
            min: min(b),
            max: max(b),
        },
        TrackSize::MinMax(lo, hi) => taffy::MinMax {
            min: min(lo),
            max: max(hi),
        },
        TrackSize::FitContent(limit) => taffy::MinMax {
            min: Min::auto(),
            max: match limit {
                LengthPercentage::Px(px) => Max::fit_content_px(px),
                LengthPercentage::Percent(p) => Max::fit_content_percent(p / 100.0),
                LengthPercentage::Calc {
                    px,
                    percent,
                    non_negative,
                } => Max::fit_content_calc(calcs.handle(px, percent, non_negative)),
            },
        },
    }
}

/// Each line's names, for taffy: none at all, or one set per line.
fn line_names(names: &[LineNames]) -> Vec<Vec<String>> {
    names
        .iter()
        .map(|names| names.iter().map(|name| name.to_string()).collect())
        .collect()
}

/// The names of a track list's lines, for taffy. taffy reads a
/// `repeat()`'s names only while it walks the list's, so a list with names
/// only in a `repeat()` gets an empty set for each of its lines.
fn template_line_names(list: &TrackList) -> Vec<Vec<String>> {
    let repeat_names = list
        .tracks
        .iter()
        .any(|track| matches!(track, GridTrack::Repeat(_, _, names) if !names.is_empty()));
    if list.line_names.is_empty() && repeat_names {
        #[expect(clippy::arithmetic_side_effects, reason = "a Vec's length, plus one")]
        let lines = list.tracks.len() + 1;
        vec![Vec::new(); lines]
    } else {
        line_names(&list.line_names)
    }
}

/// `grid-template-areas`, for taffy, whose lines are 16-bit: a template
/// past that many saturates, and taffy takes no more tracks than its own
/// limit (10000) anyway.
fn template_areas(areas: &GridTemplateAreas) -> taffy::GridTemplateAreas<String> {
    let line = base::numerics::saturated_cast::<u16, u32>;
    taffy::GridTemplateAreas {
        areas: areas
            .areas
            .iter()
            .map(|area| taffy::GridTemplateArea {
                name: area.name.to_string(),
                row_start: line(area.row_start),
                row_end: line(area.row_end),
                column_start: line(area.column_start),
                column_end: line(area.column_end),
            })
            .collect(),
        row_count: line(areas.rows),
        column_count: line(areas.columns),
    }
}

/// A `<grid-line>` for taffy, whose lines and spans are 16-bit.
fn grid_line(line: &GridLine) -> taffy::GridPlacement<String> {
    let line_number = base::numerics::saturated_cast::<i16, i32>;
    let span = base::numerics::saturated_cast::<u16, u32>;
    match line {
        GridLine::Auto => taffy::GridPlacement::Auto,
        GridLine::Line(n) => taffy::style_helpers::line(line_number(*n)),
        GridLine::Span(n) => taffy::GridPlacement::Span(span(*n)),
        // An ident alone: an area's edge first, else the first line of that
        // name (css-grid 2, "Line-based Placement").
        GridLine::Ident(name) => taffy::GridPlacement::NamedLine(name.to_string(), 0),
        GridLine::Named(name, n) => {
            taffy::GridPlacement::NamedLine(name.to_string(), line_number(*n))
        }
        GridLine::NamedSpan(name, n) => taffy::GridPlacement::NamedSpan(name.to_string(), span(*n)),
    }
}

/// The document's size for the layer: the viewport, grown to include every
/// box and line, except what a clipping box hides. Transforms are not
/// taken into account.
pub fn document_size(layout_view: &BoxFragment) -> SizeF {
    let mut size = layout_view.size;
    extend_overflow(layout_view, PointF::new(0.0, 0.0), &mut size);
    size
}

/// A box's scrollable overflow (css-overflow 3), from its padding box's
/// origin, as Blink's `ScrollableOverflowCalculator` computes it: the
/// padding box, grown to include its children's border boxes (and what
/// overflows them, unless they clip it) and its in-flow content (the lines
/// and the children's margin boxes) with the end padding around it: a
/// child's negative margin takes the padding in. Transforms are not taken into account.
pub fn scrollable_overflow(fragment: &BoxFragment) -> SizeF {
    let (b, p) = (&fragment.border, &fragment.padding);
    let mut overflow = SizeF::new(
        fragment.size.width - b.left - b.right,
        fragment.size.height - b.top - b.bottom,
    );
    // A box of auto height ends where its content does: a child's margin
    // box sticks out of it only if its block-end margin collapsed through
    // (margin collapsing), and is then this box's own margin, outside it.
    // Blink carries that on in the margin strut, not the in-flow bounds.
    let padding_end =
        (fragment.style.height == computed_style::Size::Auto).then_some(overflow.height);
    // The in-flow content's bounds (Blink's `AddInflowBounds`), from the
    // padding box's origin.
    let origin = PointF::new(-b.left, -b.top);
    let mut inflow = SizeF::new(0.0, 0.0);
    for line in &fragment.lines {
        inflow.width = inflow.width.max(origin.x + line.rect.right());
        inflow.height = inflow.height.max(origin.y + line.rect.bottom());
    }
    for child in &fragment.children {
        let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
        let child = &child.fragment;
        // Fixed to the viewport: it doesn't scroll with the document, so
        // it is no part of what can be scrolled to.
        if child.fixed_to_viewport {
            continue;
        }
        // An out-of-flow box reaches as far as its border box and what
        // overflows it, but isn't in-flow content: the end padding isn't
        // added after it (Blink's `ScrollableOverflowCalculator` adds it to
        // the in-flow bounds only).
        if child.style.is_out_of_flow() {
            extend_overflow(child, at, &mut overflow);
            continue;
        }
        // A box with no area at all doesn't reach anywhere of its own
        // (Blink's `ScrollableOverflowCalculator` skips an empty rect);
        // one that is flat on an axis still reaches along the other. What
        // is inside it counts either way, through `extend_overflow`.
        if child.size.width <= 0.0 && child.size.height <= 0.0 {
            extend_overflow(child, at, &mut overflow);
            continue;
        }
        let border_end = at.y + child.size.height;
        let mut margin_end = border_end + child.margin.bottom;
        if let Some(end) = padding_end {
            margin_end = margin_end.min(end.max(border_end));
        }
        inflow.width = inflow
            .width
            .max(at.x + child.size.width + child.margin.right);
        inflow.height = inflow.height.max(margin_end);
        extend_overflow(child, at, &mut overflow);
    }
    overflow.width = overflow.width.max(inflow.width + p.right);
    overflow.height = overflow.height.max(inflow.height + p.bottom);
    overflow
}

/// Grows `size` (from some origin) to include `fragment`'s border box at
/// `origin`, and its lines and descendants unless it clips them.
fn extend_overflow(fragment: &BoxFragment, origin: PointF, size: &mut SizeF) {
    if fragment.fixed_to_viewport {
        return;
    }
    if fragment.size.width > 0.0 || fragment.size.height > 0.0 {
        size.width = size.width.max(origin.x + fragment.size.width);
        size.height = size.height.max(origin.y + fragment.size.height);
    }
    // What a box clips, or with layout containment holds as ink overflow
    // (css-contain 2), reaches nowhere outside it.
    if clips_content(&fragment.style)
        || applied_containment(&fragment.style).contains(computed_style::Contain::LAYOUT)
    {
        return;
    }
    for line in &fragment.lines {
        size.width = size.width.max(origin.x + line.rect.right());
        size.height = size.height.max(origin.y + line.rect.bottom());
    }
    for child in &fragment.children {
        let offset = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
        extend_overflow(&child.fragment, offset, size);
    }
}

/// Border widths `widths` as taffy has them.
pub(crate) fn border_rect(widths: Sides<f32>) -> taffy::Rect<taffy::LengthPercentage> {
    taffy::Rect {
        top: taffy::LengthPercentage::length(widths.top),
        right: taffy::LengthPercentage::length(widths.right),
        bottom: taffy::LengthPercentage::length(widths.bottom),
        left: taffy::LengthPercentage::length(widths.left),
    }
}

/// See [`LayoutBox::block_item_style`].
fn block_item_style(
    style: &ComputedStyle,
    taffy_style: &taffy::Style,
) -> Option<Box<taffy::Style>> {
    if table::TablePart::of(style.display) != Some(table::TablePart::Table)
        || style.is_out_of_flow()
    {
        return None;
    }
    let mut taffy_style = taffy_style.clone();
    taffy_style.size.width = taffy::Dimension::auto();
    taffy_style.min_size.width = taffy::LengthPercentageAuto::auto();
    taffy_style.max_size.width = taffy::LengthPercentageAuto::auto();
    Some(Box::new(taffy_style))
}

/// What an item of `-webkit-box` `container` goes by instead of the flex
/// longhands (Blink's `ResolvedFlexGrow`, `ResolvedFlexShrink`,
/// `ShouldApplyAutoMinSize` and `FlexLayoutAlgorithm`'s flex base size):
/// `-webkit-box-flex` grows and shrinks it, the box's `-webkit-box-align`
/// aligns it whatever its `align-self`, a `min-width` or `min-height` of
/// `auto` is 0, and in a horizontal box an `auto` width is `fit-content`:
/// its text wraps in the box rather than run past it. (In a vertical one
/// that is its content's height anyway; `-webkit-box-ordinal-group` orders
/// it: `order_children`.)
fn webkit_box_item(
    style: &ComputedStyle,
    container: &ComputedStyle,
    taffy_style: &mut taffy::Style,
) {
    if container.rare.box_orient == computed_style::BoxOrient::Horizontal
        && style.width == Size::Auto
    {
        taffy_style.flex_basis = taffy::Dimension::fit_content();
    }
    let flex = style.rare.box_flex.max(0.0);
    taffy_style.flex_grow = flex;
    taffy_style.flex_shrink = flex;
    taffy_style.align_self = None;
    let zero = |min: &mut taffy::LengthPercentageAuto| {
        if min.is_auto() {
            *min = taffy::LengthPercentageAuto::length(0.0);
        }
    };
    zero(&mut taffy_style.min_size.width);
    zero(&mut taffy_style.min_size.height);
}

/// [`taffy_style`] for a box of `role`. The root element's box establishes
/// a block formatting context (CSS 2.2 §8.3.1: the root's margins don't
/// collapse), so `<body>`'s margin stays inside it, as in Chrome; paint
/// containment is how taffy says so (its other effect, on taffy's own
/// overflow, machopo doesn't read).
fn role_taffy_style(style: &ComputedStyle, role: BoxRole, calcs: &CalcValues) -> taffy::Style {
    let mut taffy_style = taffy_style(style, calcs);
    if role == BoxRole::RootElement {
        taffy_style.contain = taffy::Contain::PAINT;
    }
    taffy_style
}

/// [`role_taffy_style`] for a button: a flow root (paint containment, as
/// the root element's), and in a block (not `in_flex_or_grid`) an `auto`
/// width is fit-content, as for a float (Blink's
/// `LayoutBox::AutoWidthShouldFitContent`). Out of flow it is sized as any
/// box: taffy's absolute layout takes fit-content as `auto`, the insets'
/// width.
fn button_taffy_style(
    style: &ComputedStyle,
    role: BoxRole,
    in_flex_or_grid: bool,
    calcs: &CalcValues,
) -> taffy::Style {
    let mut taffy_style = role_taffy_style(style, role, calcs);
    taffy_style.contain = taffy::Contain::PAINT;
    if !in_flex_or_grid && style.width == Size::Auto {
        taffy_style.size.width = taffy::Dimension::fit_content();
    }
    taffy_style
}

fn taffy_style(style: &ComputedStyle, calcs: &CalcValues) -> taffy::Style {
    use taffy::{Dimension, LengthPercentageAuto as Lpa};

    let lp = |v: LengthPercentage| match v {
        LengthPercentage::Px(px) => taffy::LengthPercentage::length(px),
        LengthPercentage::Percent(p) => taffy::LengthPercentage::percent(p / 100.0),
        LengthPercentage::Calc {
            px,
            percent,
            non_negative,
        } => taffy::LengthPercentage::calc(calcs.handle(px, percent, non_negative)),
    };
    let lpa = |v: LengthPercentageOrAuto| match v {
        LengthPercentageOrAuto::Auto => Lpa::auto(),
        LengthPercentageOrAuto::LengthPercentage(v) => lp(v).into(),
    };
    let dimension = |v: LengthPercentageOrAuto| match v {
        LengthPercentageOrAuto::Auto => Dimension::auto(),
        LengthPercentageOrAuto::LengthPercentage(v) => lp(v).into(),
    };
    // taffy resolves the keywords against the content; in the block axis
    // (and where it doesn't), they are `auto`.
    let sized = |v: Size| match v {
        Size::Auto => Dimension::auto(),
        Size::LengthPercentage(v) => dimension(LengthPercentageOrAuto::LengthPercentage(v)),
        Size::MinContent => Dimension::min_content(),
        Size::MaxContent => Dimension::max_content(),
        Size::FitContent => Dimension::fit_content(),
    };
    // taffy spells `none` as `auto` here.
    let max = |v: LengthPercentageOrNone| match v {
        LengthPercentageOrNone::None => Lpa::auto(),
        LengthPercentageOrNone::LengthPercentage(v) => {
            lpa(LengthPercentageOrAuto::LengthPercentage(v))
        }
    };
    let content_alignment = |v: ContentAlignment| {
        use taffy::AlignContent as A;
        match v {
            ContentAlignment::Normal => A::NORMAL,
            ContentAlignment::Start => A::START,
            ContentAlignment::End => A::END,
            ContentAlignment::FlexStart => A::FLEX_START,
            ContentAlignment::FlexEnd => A::FLEX_END,
            ContentAlignment::Center => A::CENTER,
            ContentAlignment::Stretch => A::STRETCH,
            ContentAlignment::SpaceBetween => A::SPACE_BETWEEN,
            ContentAlignment::SpaceAround => A::SPACE_AROUND,
            ContentAlignment::SpaceEvenly => A::SPACE_EVENLY,
        }
    };
    let item_alignment = |v: ItemAlignment| {
        use taffy::AlignItems as A;
        match v {
            ItemAlignment::Normal => A::NORMAL,
            ItemAlignment::Stretch => A::STRETCH,
            ItemAlignment::Start | ItemAlignment::SelfStart => A::START,
            ItemAlignment::End | ItemAlignment::SelfEnd => A::END,
            ItemAlignment::FlexStart => A::FLEX_START,
            ItemAlignment::FlexEnd => A::FLEX_END,
            ItemAlignment::Center => A::CENTER,
            ItemAlignment::Baseline => A::BASELINE,
        }
    };
    let mut taffy_style = taffy::Style {
        display: match style.display {
            Display::Flex | Display::InlineFlex | Display::WebkitBox | Display::WebkitInlineBox => {
                taffy::Display::Flex
            }
            Display::Grid | Display::InlineGrid => taffy::Display::Grid,
            _ => taffy::Display::Block,
        },
        grid_template_columns: style
            .rare
            .grid_template_columns
            .tracks
            .iter()
            .map(|track| grid_track(track, calcs))
            .collect(),
        grid_template_rows: style
            .rare
            .grid_template_rows
            .tracks
            .iter()
            .map(|track| grid_track(track, calcs))
            .collect(),
        grid_template_column_names: template_line_names(&style.rare.grid_template_columns),
        grid_template_row_names: template_line_names(&style.rare.grid_template_rows),
        grid_template_areas: style
            .rare
            .grid_template_areas
            .as_deref()
            .map(template_areas),
        grid_auto_columns: style
            .rare
            .grid_auto_columns
            .iter()
            .map(|size| track_size(size, calcs))
            .collect(),
        grid_auto_rows: style
            .rare
            .grid_auto_rows
            .iter()
            .map(|size| track_size(size, calcs))
            .collect(),
        grid_auto_flow: match style.rare.grid_auto_flow {
            GridAutoFlow::Row => taffy::GridAutoFlow::Row,
            GridAutoFlow::Column => taffy::GridAutoFlow::Column,
            GridAutoFlow::RowDense => taffy::GridAutoFlow::RowDense,
            GridAutoFlow::ColumnDense => taffy::GridAutoFlow::ColumnDense,
        },
        grid_row: taffy::Line {
            start: grid_line(&style.rare.grid_row_start),
            end: grid_line(&style.rare.grid_row_end),
        },
        grid_column: taffy::Line {
            start: grid_line(&style.rare.grid_column_start),
            end: grid_line(&style.rare.grid_column_end),
        },
        justify_items: item_alignment(style.justify_items),
        justify_self: style.justify_self.map(item_alignment),
        flex_direction: match style.flex_direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
            FlexDirection::Column => taffy::FlexDirection::Column,
            FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
        },
        flex_wrap: match style.flex_wrap {
            FlexWrap::Nowrap => taffy::FlexWrap::NoWrap,
            FlexWrap::Wrap => taffy::FlexWrap::Wrap,
            FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        },
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        flex_basis: match style.flex_basis {
            FlexBasis::Content => Dimension::content(),
            FlexBasis::Size(size) => sized(size),
        },
        justify_content: content_alignment(style.justify_content),
        align_content: content_alignment(style.align_content),
        align_items: item_alignment(style.align_items),
        align_self: style.align_self.map(item_alignment),
        gap: taffy::Size {
            width: lp(style.column_gap),
            height: lp(style.row_gap),
        },
        box_sizing: match style.box_sizing {
            BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
            BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
        },
        size: taffy::Size {
            width: sized(style.width),
            height: sized(style.height),
        },
        min_size: taffy::Size {
            width: lpa(style.min_width),
            height: lpa(style.min_height),
        },
        max_size: taffy::Size {
            width: max(style.max_width),
            height: max(style.max_height),
        },
        margin: taffy::Rect {
            top: lpa(style.margin.top),
            right: lpa(style.margin.right),
            bottom: lpa(style.margin.bottom),
            left: lpa(style.margin.left),
        },
        // An out-of-flow box goes up to its containing block, which taffy
        // lays it out against; a static one's inset is ignored. A sticky
        // one is laid out where the flow puts it, unmoved: its inset
        // moves it as its scrollport scrolls, after layout.
        position: match style.position {
            Position::Static => taffy::Position::Static,
            Position::Relative | Position::Sticky => taffy::Position::Relative,
            Position::Absolute => taffy::Position::Absolute,
            Position::Fixed => taffy::Position::Fixed,
        },
        inset: if style.position == Position::Sticky {
            taffy::Rect::auto()
        } else {
            taffy::Rect {
                top: lpa(style.inset.top),
                right: lpa(style.inset.right),
                bottom: lpa(style.inset.bottom),
                left: lpa(style.inset.left),
            }
        },
        // Read by taffy's block layout only: a flex or grid item doesn't
        // float.
        float: match style.float {
            Float::None => taffy::Float::None,
            Float::Left => taffy::Float::Left,
            Float::Right => taffy::Float::Right,
        },
        clear: match style.clear {
            Clear::None => taffy::Clear::None,
            Clear::Left => taffy::Clear::Left,
            Clear::Right => taffy::Clear::Right,
            Clear::Both => taffy::Clear::Both,
        },
        padding: taffy::Rect {
            top: lp(style.padding.top),
            right: lp(style.padding.right),
            bottom: lp(style.padding.bottom),
            left: lp(style.padding.left),
        },
        // A scroll container establishes a block formatting context, and
        // a flex item's automatic minimum size is 0; a `clip` axis neither
        // (taffy's `Clip` isn't a scroll container).
        overflow: {
            let axis = |overflow| match overflow {
                Overflow::Visible => taffy::Overflow::Visible,
                Overflow::Clip => taffy::Overflow::Clip,
                Overflow::Hidden | Overflow::Auto | Overflow::Scroll => taffy::Overflow::Hidden,
            };
            taffy::Point {
                x: axis(style.overflow_x),
                y: axis(style.overflow_y),
            }
        },
        border: border_rect(style.border_width),
        ..taffy::Style::DEFAULT
    };
    // A `-webkit-box` is a flex container that goes by its own longhands
    // (Blink's `FlexLayoutAlgorithm` for `IsDeprecatedFlexbox`): on one
    // line, in the direction of `-webkit-box-orient` and
    // `-webkit-box-direction`, packed as `-webkit-box-pack` says, with every
    // item aligned as `-webkit-box-align` says (`webkit_box_item`).
    if style.display.is_webkit_box() {
        use computed_style::{BoxAlign, BoxDirection, BoxOrient, BoxPack};
        let rare = &style.rare;
        taffy_style.flex_direction = match (rare.box_orient, rare.box_direction) {
            (BoxOrient::Horizontal, BoxDirection::Normal) => taffy::FlexDirection::Row,
            (BoxOrient::Horizontal, BoxDirection::Reverse) => taffy::FlexDirection::RowReverse,
            (BoxOrient::Vertical, BoxDirection::Normal) => taffy::FlexDirection::Column,
            (BoxOrient::Vertical, BoxDirection::Reverse) => taffy::FlexDirection::ColumnReverse,
        };
        taffy_style.flex_wrap = taffy::FlexWrap::NoWrap;
        taffy_style.justify_content = match rare.box_pack {
            BoxPack::Start => taffy::JustifyContent::FLEX_START,
            BoxPack::Center => taffy::JustifyContent::CENTER,
            BoxPack::End => taffy::JustifyContent::FLEX_END,
            BoxPack::Justify => taffy::JustifyContent::SPACE_BETWEEN,
        };
        taffy_style.align_items = match rare.box_align {
            BoxAlign::Stretch => taffy::AlignItems::STRETCH,
            BoxAlign::Start => taffy::AlignItems::FLEX_START,
            BoxAlign::Center => taffy::AlignItems::CENTER,
            BoxAlign::End => taffy::AlignItems::FLEX_END,
            BoxAlign::Baseline => taffy::AlignItems::BASELINE,
        };
    }
    // A `flow-root` establishes a block formatting context: paint
    // containment is how taffy says so, as for the root element's box.
    if style.display == Display::FlowRoot {
        taffy_style.contain = taffy::Contain::PAINT;
    }
    // Layout and paint containment: an independent formatting context,
    // and with layout containment no baseline (size containment is
    // `BoxTree::lay_out_box`'s; taffy hasn't it).
    let contain = applied_containment(style);
    if contain.contains(computed_style::Contain::LAYOUT) {
        taffy_style.contain = taffy_style.contain.union(taffy::Contain::LAYOUT);
    }
    if contain.contains(computed_style::Contain::PAINT) {
        taffy_style.contain = taffy_style.contain.union(taffy::Contain::PAINT);
    }
    // taffy's block layout neither stretches a table nor puts it in its
    // block formatting context: the table works its width out, no smaller
    // than its content. Nor does taffy size an out-of-flow table by its
    // own sizes, which would be final there: the table works them out. (A
    // flex or grid item's are its basis and the like, as taffy has them.)
    // (Its parts but its captions have no margins, nor its rows and row
    // groups padding: the table places them, never taffy.) A cell is as
    // tall as its content, whatever its `max-height` (Blink's).
    match table::TablePart::of(style.display) {
        Some(table::TablePart::Table) => {
            taffy_style.item_is_table = true;
            // Collapsed borders: a table has no padding (css-tables 3).
            if style.border_collapse == BorderCollapse::Collapse {
                taffy_style.padding = taffy::Rect::zero();
            }
            if style.is_out_of_flow() {
                taffy_style.size = taffy::Size::auto();
                taffy_style.min_size = taffy::Size::auto();
                taffy_style.max_size = taffy::Size::auto();
            }
        }
        Some(table::TablePart::Cell) => {
            taffy_style.max_size.height = taffy::LengthPercentageAuto::auto();
        }
        _ => {}
    }
    taffy_style
}

/// An inline formatting context's atomic inlines and floats, as the box
/// tree lays them out, its floats placed in the block formatting context
/// `ctx` (Blink's `ExclusionSpace`, which `LineBreaker::HandleFloat`
/// positions them in): from the border box of the box it is for, its
/// content box `inset` into it.
struct InlineChildren<'t, 'c, 'b> {
    tree: &'t mut BoxTree,
    ctx: &'c mut BlockContext<'b>,
    inset: taffy::Rect<f32>,
}

impl<'t, 'c, 'b> InlineChildren<'t, 'c, 'b> {
    fn new(tree: &'t mut BoxTree, ctx: &'c mut BlockContext<'b>, inset: taffy::Rect<f32>) -> Self {
        // A float's containing block is the content box.
        ctx.apply_content_box_inset([inset.left, inset.right]);
        Self { tree, ctx, inset }
    }
}

impl AtomicInlines for InlineChildren<'_, '_, '_> {
    fn count_line_break(&mut self) {
        self.tree.count_line_break();
    }

    fn count_line_broken_again(&mut self) {
        self.tree.count_line_broken_again();
    }

    fn lay_out(
        &mut self,
        id: taffy::NodeId,
        space: AvailableSpace,
        height: Option<f32>,
    ) -> AtomicLayout {
        self.tree.lay_out(id, space, height)
    }

    fn width(&mut self, id: taffy::NodeId, space: AvailableSpace) -> f32 {
        self.tree.width(id, space)
    }

    fn float_widths(&mut self, floats: &[taffy::NodeId], space: AvailableSpace) -> f32 {
        self.tree.float_widths(floats, space)
    }

    fn places_floats(&self) -> bool {
        true
    }

    fn place_float(&mut self, id: taffy::NodeId, size: SizeF, top: f32) -> PointF {
        let style = &self.tree.boxes[usize::from(id)].taffy_style;
        let direction = style.float.float_direction().expect("a float's box floats");
        let at = self.ctx.place_floated_box(
            taffy::Size {
                width: size.width,
                height: size.height,
            },
            self.inset.top + top,
            direction,
            style.clear,
            false,
        );
        PointF::new(at.x - self.inset.left, at.y - self.inset.top)
    }

    fn float_bands(&self) -> FloatBands {
        float_bands(self.ctx, self.inset)
    }
}

/// The width an atomic inline is measured in: the room it has, or the
/// intrinsic size its `width` names (`min-content`, `max-content`; as
/// taffy's block layout sizes a float, and Blink's
/// `ComputeInlineSizeFromKeyword`). `fit-content` is the room, as `auto`
/// is: the shrink-to-fit width.
fn keyword_space(style: &ComputedStyle, space: AvailableSpace) -> AvailableSpace {
    match style.width {
        Size::MinContent => AvailableSpace::MinContent,
        Size::MaxContent => AvailableSpace::MaxContent,
        _ => space,
    }
}

impl AtomicInlines for BoxTree {
    fn count_line_break(&mut self) {
        self.last_layout.lines_broken = self.last_layout.lines_broken.wrapping_add(1);
    }

    fn count_line_broken_again(&mut self) {
        let update = &mut self.last_layout;
        update.lines_broken_again_beside_floats =
            update.lines_broken_again_beside_floats.wrapping_add(1);
    }

    fn float_widths(&mut self, floats: &[taffy::NodeId], space: AvailableSpace) -> f32 {
        // As taffy's block layout adds up a block's floats.
        let mut widths = FloatIntrinsicWidthCalculator::new(space);
        for &float in floats {
            let width = self.width(float, space);
            let style = &self.boxes[usize::from(float)].taffy_style;
            if let Some(direction) = style.float.float_direction() {
                widths.add_float(width, direction, style.clear);
            }
        }
        widths.result()
    }

    fn width(&mut self, id: taffy::NodeId, space: AvailableSpace) -> f32 {
        let index = usize::from(id);
        let style = self.boxes[index].style.clone();
        let edge = Edge::of(&style, 0.0);
        let margins = edge.margin.left + edge.margin.right;
        let b = &self.boxes[index];
        let sizing = b
            .replaced
            .as_ref()
            .or(b.control.as_ref().and_then(|c| c.sizing.as_ref()));
        if let Some(replaced) = sizing {
            // As `lay_out` sizes it, but a percentage width as 0 in a
            // min-content size: a compressible replaced element.
            let percent_base = (space == AvailableSpace::MinContent).then_some(0.0);
            return edge
                .outset(replaced.content_size(&style, percent_base), true)
                .width;
        }
        let output = self.compute_child_layout(
            id,
            LayoutInput {
                known_dimensions: taffy::Size::NONE,
                known_dimensions_are_definite: taffy::Size {
                    width: true,
                    height: true,
                },
                parent_size: taffy::Size::NONE,
                available_space: taffy::Size {
                    width: keyword_space(&style, space),
                    height: AvailableSpace::MaxContent,
                },
                sizing_mode: SizingMode::InherentSize,
                run_mode: RunMode::ComputeSize,
                axis: RequestedAxis::Horizontal,
                vertical_margins_are_collapsible: taffy::Line::FALSE,
            },
        );
        output.size.width + margins
    }

    fn lay_out(
        &mut self,
        id: taffy::NodeId,
        space: AvailableSpace,
        height: Option<f32>,
    ) -> AtomicLayout {
        let index = usize::from(id);
        let width = match space {
            AvailableSpace::Definite(width) => Some(width),
            _ => None,
        };
        let style = self.boxes[index].style.clone();
        // `auto` margins are 0 (CSS 2 § 10.3.2, § 10.3.9); percentages are
        // of the containing block's width, or 0 with none.
        let edge = Edge::of(&style, width.unwrap_or(0.0));
        let m = edge.margin;
        let with_margin = |border_box: SizeF| {
            SizeF::new(
                border_box.width + m.left + m.right,
                border_box.height + m.top + m.bottom,
            )
        };
        self.boxes[index].oof_candidates = OofCandidates::NONE;
        if let Some(replaced) = &self.boxes[index].replaced {
            // Its size from its style and image (CSS 2 § 10.3.2, § 10.6.2),
            // a percentage width as `auto` in a max-content size (css-sizing
            // 3, "cyclic percentage"). Its bottom margin edge is on the
            // baseline (§ 10.8.1).
            let border_box = edge.outset(replaced.content_size(&style, width), false);
            self.set_atomic_layout(index, border_box, &edge);
            return AtomicLayout {
                margin_box: with_margin(border_box),
                baseline: None,
            };
        }
        // As taffy's block layout lays out a child of unknown size: as
        // wide as its content in the room left by its margins (the
        // shrink-to-fit width of CSS 2 § 10.3.9; a flex or grid container
        // is as wide as its content the same way). The room taffy takes is
        // the border box's: the margins are off it already.
        let margins = m.left + m.right;
        let input = |known_width, run_mode, axis| LayoutInput {
            known_dimensions: taffy::Size {
                width: known_width,
                height: None,
            },
            known_dimensions_are_definite: taffy::Size {
                width: true,
                height: true,
            },
            parent_size: taffy::Size { width, height },
            available_space: taffy::Size {
                width: match keyword_space(&style, space) {
                    AvailableSpace::Definite(room) => {
                        AvailableSpace::Definite((room - margins).max(0.0))
                    }
                    intrinsic => intrinsic,
                },
                height: AvailableSpace::MaxContent,
            },
            sizing_mode: SizingMode::InherentSize,
            run_mode,
            axis,
            vertical_margins_are_collapsible: taffy::Line::FALSE,
        };
        // Its width first, then laid out at that width, as a block-level
        // box is: what resolves against its width (a grid's percentage
        // gaps) does against that. (A form control with content, a text
        // control's shadow tree's, is its own size still: `compute_content`
        // sizes it as it does one without.)
        let measured = self.compute_child_layout(
            id,
            input(None, RunMode::ComputeSize, RequestedAxis::Horizontal),
        );
        let mut output = self.compute_child_layout(
            id,
            input(
                Some(measured.size.width),
                RunMode::PerformLayout,
                RequestedAxis::Both,
            ),
        );
        self.boxes[index].oof_candidates = output.oof_candidates.take();
        let border_box = SizeF::new(output.size.width, output.size.height);
        self.set_atomic_layout(index, border_box, &edge);
        // A flex or grid container's first baseline (css-align 3, "first
        // baseline set"), which taffy gives. An inline-block's last line
        // box's; its bottom margin edge without one, or if it clips its
        // overflow (CSS 2 § 10.8.1).
        let button = self.boxes[index].button;
        let baseline = if let Some(control) = &self.boxes[index].control {
            control.baseline(border_box, &edge)
        } else if style.display.is_flexible_or_grid()
            || table::TablePart::of(style.display) == Some(table::TablePart::Table)
        {
            output.baselines.first
        } else if button {
            // An empty button's is its content box's top (Chrome's).
            self.last_baseline(index).or(Some(
                edge.border.top + edge.padding.top + self.boxes[index].content_shift,
            ))
        // A scroll container's is its bottom margin edge (CSS 2 § 10.8.1);
        // `clip` is as `visible` (Blink's `IsOverflowVisibleOrClip`). So is
        // one with layout containment, which has no baseline.
        } else if style.is_scroll_container()
            || applied_containment(&style).contains(computed_style::Contain::LAYOUT)
        {
            None
        } else {
            self.last_baseline(index)
        };
        AtomicLayout {
            margin_box: with_margin(border_box),
            baseline: baseline.map(|baseline| m.top + baseline),
        }
    }
}

impl TraversePartialTree for BoxTree {
    type ChildIter<'a> = std::iter::Copied<std::slice::Iter<'a, taffy::NodeId>>;

    fn child_ids(&self, parent: taffy::NodeId) -> Self::ChildIter<'_> {
        self.layout_children(usize::from(parent)).iter().copied()
    }

    fn child_count(&self, parent: taffy::NodeId) -> usize {
        self.layout_children(usize::from(parent)).len()
    }

    fn get_child_id(&self, parent: taffy::NodeId, index: usize) -> taffy::NodeId {
        self.layout_children(usize::from(parent))[index]
    }
}

impl LayoutPartialTree for BoxTree {
    type CoreContainerStyle<'a> = ContainerStyle<'a>;
    // taffy's default with `std` (named grid lines; unused).
    type CustomIdent = String;

    fn get_core_container_style(&self, id: taffy::NodeId) -> ContainerStyle<'_> {
        let b = &self.boxes[usize::from(id)];
        ContainerStyle {
            style: &b.taffy_style,
            contains_out_of_flow: contains_out_of_flow(&b.style, b.role == BoxRole::RootElement),
        }
    }

    fn set_unrounded_layout(&mut self, id: taffy::NodeId, layout: &Layout) {
        self.boxes[usize::from(id)].layout = *layout;
    }

    /// A `calc()` of a length and a percentage (see `calc`).
    fn resolve_calc_value(&self, handle: *const (), basis: f32) -> f32 {
        self.calcs.resolve(handle, basis)
    }

    fn compute_child_layout(&mut self, id: taffy::NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_box(id, inputs, None)
    }
}

impl CacheTree for BoxTree {
    fn cache_get(&mut self, id: taffy::NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.boxes[usize::from(id)].cache.get(input)
    }

    fn cache_store(&mut self, id: taffy::NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.boxes[usize::from(id)].cache.store(input, output);
    }

    fn cache_clear(&mut self, id: taffy::NodeId) {
        self.boxes[usize::from(id)].cache.clear();
    }
}

impl LayoutBlockContainer for BoxTree {
    type BlockContainerStyle<'a> = &'a taffy::Style;
    type BlockItemStyle<'a> = &'a taffy::Style;

    fn get_block_container_style(&self, id: taffy::NodeId) -> &taffy::Style {
        &self.boxes[usize::from(id)].taffy_style
    }

    fn get_block_child_style(&self, id: taffy::NodeId) -> &taffy::Style {
        let b = &self.boxes[usize::from(id)];
        match &b.block_item_style {
            Some(style) if self.measuring_width => style,
            _ => &b.taffy_style,
        }
    }

    fn compute_block_child_layout(
        &mut self,
        id: taffy::NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_box(id, inputs, block_ctx)
    }
}

impl LayoutGridContainer for BoxTree {
    type GridContainerStyle<'a> = &'a taffy::Style;
    type GridItemStyle<'a> = &'a taffy::Style;

    fn get_grid_container_style(&self, id: taffy::NodeId) -> &taffy::Style {
        &self.boxes[usize::from(id)].taffy_style
    }

    fn get_grid_child_style(&self, id: taffy::NodeId) -> &taffy::Style {
        &self.boxes[usize::from(id)].taffy_style
    }

    fn set_detailed_grid_info(&mut self, id: taffy::NodeId, info: DetailedGridInfo<String>) {
        self.boxes[usize::from(id)].grid_info = DetailedLayoutInfo::Grid(Box::new(info));
    }
}

impl LayoutContainingBlock for BoxTree {
    type OofItemStyle<'a> = &'a taffy::Style;

    fn get_oof_item_style(&self, id: taffy::NodeId) -> &taffy::Style {
        &self.boxes[usize::from(id)].taffy_style
    }

    fn clear_hoisted_children(&mut self, id: taffy::NodeId) {
        self.boxes[usize::from(id)].absolute.clear();
    }

    fn add_hoisted_children(&mut self, id: taffy::NodeId, hoisted: &[taffy::NodeId]) {
        self.boxes[usize::from(id)]
            .absolute
            .extend_from_slice(hoisted);
    }

    fn get_detailed_layout_info(&self, id: taffy::NodeId) -> &DetailedLayoutInfo<String> {
        &self.boxes[usize::from(id)].grid_info
    }
}

impl LayoutFlexboxContainer for BoxTree {
    type FlexboxContainerStyle<'a> = &'a taffy::Style;
    type FlexboxItemStyle<'a> = &'a taffy::Style;

    fn get_flexbox_container_style(&self, id: taffy::NodeId) -> &taffy::Style {
        &self.boxes[usize::from(id)].taffy_style
    }

    fn get_flexbox_child_style(&self, id: taffy::NodeId) -> &taffy::Style {
        &self.boxes[usize::from(id)].taffy_style
    }
}
