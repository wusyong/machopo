//! `ComputedStyle` (`blink/renderer/core/style`): the computed value of every
//! supported property for one element. `style` produces it, `layout` and
//! `paint` read it, and `dom` stores it on elements.
//!
//! **Plain data only.** `dom` keeps `Arc<ComputedStyle>` on GC nodes
//! in a `elise::External`, so the last `Arc` may be dropped by a GC
//! finalizer, on a sweep's worker thread. Everything here must therefore:
//!
//! - hold no GC pointers or roots, and be `Send + Sync + 'static` (checked
//!   by `External`'s bounds): no `Rc`, `Cell`, `RefCell`, raw pointers;
//! - have no hand-written `Drop`, and no interning or global / thread-local
//!   caches (their `Drop` would lock or touch thread-locals).
//!
//! Changes to these types are reviewed as `External`'s values are; see
//! elise's soundness argument. Once a style needs to hold a GC object,
//! `ComputedStyle` itself becomes a GC object instead. Images don't: a
//! `background-image` is only its URL, and the images, `<img>`'s too, live
//! in the renderer's per-document table.
//!
//! Lengths are in CSS px. Percentages are kept as written (`50%` is
//! `Percent(50.0)`) and resolved by layout. `currentcolor` is kept as
//! [`StyleColor::CurrentColor`] and resolved against `color` where it is
//! used, as Blink's `StyleColor` does, so that `inherit` copies the keyword
//! rather than the parent's color.

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

mod animation;
mod path_data;
mod variables;

use std::sync::Arc;

pub use crate::animation::{
    AnimationName, AnimationPlayState, AnimationRangeBoundary, AnimationTimeline, AnimationTiming,
    Animations, EasingFunction, FillMode, LinearStop, PlaybackDirection, StepPosition,
    TransitionBehavior, TransitionProperty, Transitions,
};
pub use crate::variables::StyleVariables;

use gfx::{Color, FillRule, RectF, SizeF, Transform};

pub use crate::path_data::{absolute_path, append_path_data, segment_is_abs};

/// The longest length a computed value holds, either way, in px: Blink's
/// `kMaxValueForCssLength` (`INT_MAX / 64 - 2`, what its `LayoutUnit`
/// holds) as an f32 has it. Layout, paint and text never get an infinite
/// length, or one whose sum with another is.
pub const MAX_CSS_LENGTH: f32 = 33_554_428.0;

/// The biggest font size, in px (Blink's `kMaximumAllowedFontSize`).
pub const MAX_FONT_SIZE: f32 = 10_000.0;

/// `px` as a computed length holds it (Blink's `ClampToCSSLengthRange`):
/// within [`MAX_CSS_LENGTH`] either way, NaN 0. Percentages too: what they
/// are of is in that range, and taffy multiplies them itself.
pub fn clamp_css_length(px: f32) -> f32 {
    if px.is_nan() {
        0.0
    } else {
        px.clamp(-MAX_CSS_LENGTH, MAX_CSS_LENGTH)
    }
}

/// `<length-percentage>`, computed: absolute lengths in px, within
/// [`clamp_css_length`]'s range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPercentage {
    Px(f32),
    Percent(f32),
    /// A `calc()` of both: `px` plus `percent` of the basis, which isn't
    /// known until it is used (Blink's `CalculationValue` as
    /// `PixelsAndPercent`). `non_negative` for properties that take no
    /// negative value: the result is clamped to 0 then.
    Calc {
        px: f32,
        percent: f32,
        non_negative: bool,
    },
}

impl LengthPercentage {
    pub const ZERO: Self = Self::Px(0.0);

    /// Its length in px, percentages of `basis`: in the range a computed
    /// length is (Blink's `CalculationValue::Evaluate`, then its
    /// `LayoutUnit`).
    pub fn resolve(self, basis: f32) -> f32 {
        let v = match self {
            Self::Px(px) => px,
            Self::Percent(percent) => percent / 100.0 * basis,
            Self::Calc {
                px,
                percent,
                non_negative,
            } => {
                let v = px + percent / 100.0 * basis;
                if non_negative { v.max(0.0) } else { v }
            }
        };
        clamp_css_length(v)
    }

    /// Its length in px if it needs no basis: a length, not a percentage
    /// or a `calc()` with one.
    pub fn px(self) -> Option<f32> {
        match self {
            Self::Px(px) => Some(px),
            Self::Percent(_) | Self::Calc { .. } => None,
        }
    }

    /// Whether it depends on a basis (a percentage, or a `calc()` with
    /// one): what layout treats as `auto` where there is no definite one.
    pub fn has_percent(self) -> bool {
        !matches!(self, Self::Px(_))
    }
}

/// `auto | <length-percentage>`: `width`, `height`, `min-*`, `margin-*`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPercentageOrAuto {
    Auto,
    LengthPercentage(LengthPercentage),
}

/// `width` / `height` (css-sizing 3): `auto`, a length or percentage, or
/// an intrinsic size keyword, which layout resolves from the content. In
/// the block axis (`height`), the keywords behave as `auto`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Auto,
    LengthPercentage(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent,
}

impl Size {
    /// The length or percentage, if it is one.
    pub fn length_percentage(self) -> Option<LengthPercentage> {
        match self {
            Self::LengthPercentage(v) => Some(v),
            _ => None,
        }
    }
}

/// `flex-basis`: `content`, or a size as for `width`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FlexBasis {
    Content,
    Size(Size),
}

/// `<track-breadth>` (css-grid 2): how big a grid track wants to be. `L`
/// is the length type: computed here, as declared in `style`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackBreadth<L = LengthPercentage> {
    LengthPercentage(L),
    /// `fr`: a share of the free space.
    Flex(f32),
    Auto,
    MinContent,
    MaxContent,
}

/// `<track-size>`: a breadth, `minmax()` or `fit-content()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackSize<L = LengthPercentage> {
    Breadth(TrackBreadth<L>),
    MinMax(TrackBreadth<L>, TrackBreadth<L>),
    FitContent(L),
}

/// `repeat()`'s first argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatCount {
    Count(u16),
    AutoFill,
    AutoFit,
}

/// The names of one grid line (`[a b]`); empty for none.
pub type LineNames = Arc<[Arc<str>]>;

/// One entry of `grid-template-columns` / `-rows`: a track, or
/// `repeat()` of some, with the names of the lines around each of its
/// tracks (one more than the tracks, or none at all).
#[derive(Debug, Clone, PartialEq)]
pub enum GridTrack<L = LengthPercentage> {
    Size(TrackSize<L>),
    Repeat(RepeatCount, Arc<[TrackSize<L>]>, Arc<[LineNames]>),
}

/// `grid-template-columns` / `-rows`: the tracks, and the names of the
/// lines before each entry and after the last (one more than the entries,
/// or none at all); a `repeat()`'s last line takes the names after it too.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackList<L = LengthPercentage> {
    /// Empty for `none`.
    pub tracks: Arc<[GridTrack<L>]>,
    pub line_names: Arc<[LineNames]>,
}

impl<L> TrackList<L> {
    /// `none`.
    pub fn none() -> Self {
        Self {
            tracks: Arc::new([]),
            line_names: Arc::new([]),
        }
    }
}

impl<L> TrackList<L> {
    /// The same list with its lengths mapped by `f`.
    pub fn map<M>(&self, f: impl Fn(&L) -> M) -> TrackList<M> {
        TrackList {
            tracks: self.tracks.iter().map(|track| track.map(&f)).collect(),
            line_names: self.line_names.clone(),
        }
    }
}

/// One named area of `grid-template-areas`, between grid lines numbered
/// from 1. As many as the template has, as Blink keeps them (up to its
/// million tracks); layout takes as many as taffy does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridArea {
    pub name: Arc<str>,
    pub row_start: u32,
    pub row_end: u32,
    pub column_start: u32,
    pub column_end: u32,
}

/// `grid-template-areas` (Blink's `NamedGridAreaMap`): the named areas,
/// each a rectangle, and the size of the template, which unnamed cells
/// (`.`) may make larger than the areas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridTemplateAreas {
    pub areas: Arc<[GridArea]>,
    pub rows: u32,
    pub columns: u32,
}

impl<L> TrackBreadth<L> {
    /// The same breadth with its length mapped by `f`.
    pub fn map<M>(&self, f: impl FnOnce(&L) -> M) -> TrackBreadth<M> {
        match self {
            Self::LengthPercentage(v) => TrackBreadth::LengthPercentage(f(v)),
            Self::Flex(v) => TrackBreadth::Flex(*v),
            Self::Auto => TrackBreadth::Auto,
            Self::MinContent => TrackBreadth::MinContent,
            Self::MaxContent => TrackBreadth::MaxContent,
        }
    }
}

impl<L> TrackSize<L> {
    /// The same size with its lengths mapped by `f`.
    pub fn map<M>(&self, f: impl Fn(&L) -> M) -> TrackSize<M> {
        match self {
            Self::Breadth(b) => TrackSize::Breadth(b.map(f)),
            Self::MinMax(min, max) => TrackSize::MinMax(min.map(&f), max.map(&f)),
            Self::FitContent(v) => TrackSize::FitContent(f(v)),
        }
    }
}

impl<L> GridTrack<L> {
    /// The same track with its lengths mapped by `f`.
    pub fn map<M>(&self, f: impl Fn(&L) -> M) -> GridTrack<M> {
        match self {
            Self::Size(size) => GridTrack::Size(size.map(f)),
            Self::Repeat(count, sizes, names) => GridTrack::Repeat(
                *count,
                sizes.iter().map(|size| size.map(&f)).collect(),
                names.clone(),
            ),
        }
    }
}

/// `grid-auto-flow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridAutoFlow {
    Row,
    Column,
    RowDense,
    ColumnDense,
}

/// `grid-row-start` and the others (`<grid-line>`): `auto`, a line
/// number (not 0; negative from the end), a span of tracks, or the same
/// counting only the lines with a name (an area's name standing for its
/// `-start` or `-end` line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridLine {
    Auto,
    Line(i32),
    Span(u32),
    /// A name alone: the same line as `Named(name, 1)`, written without the
    /// number.
    Ident(Arc<str>),
    Named(Arc<str>, i32),
    NamedSpan(Arc<str>, u32),
}

/// `none | <length-percentage>`: `max-*`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LengthPercentageOrNone {
    None,
    LengthPercentage(LengthPercentage),
}

/// `display`, the values laid out so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    /// A block container that establishes a block formatting context
    /// (css-display 3, `flow-root`).
    FlowRoot,
    /// A block-level flex container.
    Flex,
    /// A block-level grid container.
    Grid,
    /// An inline-level block container: an atomic inline, laid out as a
    /// whole on a line.
    InlineBlock,
    /// An inline-level flex container, an atomic inline.
    InlineFlex,
    /// An inline-level grid container, an atomic inline.
    InlineGrid,
    /// `-webkit-box`: a block-level flex container with the `-webkit-box-*`
    /// properties for its direction, alignment and items' flexing (Blink's
    /// `EDisplay::kWebkitBox`). With a vertical `-webkit-line-clamp` it is a
    /// `flow-root` instead.
    WebkitBox,
    /// `-webkit-inline-box`: an inline-level `-webkit-box`, an atomic
    /// inline.
    WebkitInlineBox,
    /// `table` and the table-internal values: a table box, or one of its
    /// parts, which the table fixup puts in one.
    Table(TableDisplay),
    /// `inline-table`: an inline-level table box, an atomic inline.
    InlineTable,
    /// A block container with a marker (css-lists 3; Blink's
    /// `LayoutListItem`): block-level, laid out as `block`.
    ListItem,
    /// No box of its own: its children (and `::before`, `::after`) are
    /// laid out as its parent's (css-display 3). It still has a style,
    /// which they inherit. Not blockified; the root element's is `block`.
    Contents,
    None,
}

/// `table` and the table-internal `display` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableDisplay {
    Table,
    RowGroup,
    HeaderGroup,
    FooterGroup,
    Row,
    Cell,
    ColumnGroup,
    Column,
    Caption,
}

impl Display {
    /// Whether the element's box is block-level: a box in the box tree,
    /// not inline content.
    pub fn is_block_level(self) -> bool {
        matches!(
            self,
            Self::Block
                | Self::FlowRoot
                | Self::Flex
                | Self::Grid
                | Self::WebkitBox
                | Self::Table(_)
                | Self::ListItem
        )
    }

    /// Whether its children are flex or grid items (Blink's
    /// `IsDisplayFlexibleOrGridBox`): blockified, in `order`, painted whole,
    /// with their `z-index`.
    pub fn is_flexible_or_grid(self) -> bool {
        self.blockifies_children() || self.is_webkit_box()
    }

    /// Whether its children are blockified (Blink's
    /// `ComputedStyle::BlockifiesChildren`): a flex or grid container's, but
    /// not a `-webkit-box`'s, whose inline children go into anonymous
    /// items as text does.
    pub fn blockifies_children(self) -> bool {
        matches!(
            self,
            Self::Flex | Self::Grid | Self::InlineFlex | Self::InlineGrid
        )
    }

    /// Whether it is a `-webkit-box`, block-level or inline-level.
    pub fn is_webkit_box(self) -> bool {
        matches!(self, Self::WebkitBox | Self::WebkitInlineBox)
    }

    /// Whether the element generates no box at all: `none`, and the table
    /// column values, which aren't rendered (CSS 2 § 17.2; Chrome renders
    /// nothing for them outside a table either).
    pub fn generates_no_box(self) -> bool {
        matches!(
            self,
            Self::None | Self::Table(TableDisplay::Column | TableDisplay::ColumnGroup)
        )
    }

    /// Whether it is a flex container, block-level or inline-level, a
    /// `-webkit-box` too.
    pub fn is_flex(self) -> bool {
        matches!(
            self,
            Self::Flex | Self::InlineFlex | Self::WebkitBox | Self::WebkitInlineBox
        )
    }

    /// Whether it is a grid container, block-level or inline-level.
    pub fn is_grid(self) -> bool {
        matches!(self, Self::Grid | Self::InlineGrid)
    }

    /// Whether the element's box is an atomic inline (css-display): a box
    /// in the box tree, sized and placed as one piece of inline content.
    /// An inline replaced element (`<img>`) is one too, whatever its
    /// `display`: that is the element's, not the style's.
    pub fn is_atomic_inline(self) -> bool {
        matches!(
            self,
            Self::InlineBlock
                | Self::InlineFlex
                | Self::InlineGrid
                | Self::WebkitInlineBox
                | Self::InlineTable
        )
    }

    /// The block-level `display` of an element that has to be one
    /// (css-display, "blockification"; Blink's `EquivalentBlockDisplay`):
    /// a table-internal box becomes a block container.
    pub fn blockified(self) -> Self {
        match self {
            Self::Inline | Self::InlineBlock => Self::Block,
            Self::InlineFlex => Self::Flex,
            Self::InlineGrid => Self::Grid,
            Self::WebkitInlineBox => Self::WebkitBox,
            Self::InlineTable => Self::Table(TableDisplay::Table),
            Self::Table(TableDisplay::Table) => self,
            Self::Table(_) => Self::Block,
            other => other,
        }
    }
}

/// A pseudo-element whose style is computed with its element's
/// (`blink::PseudoId`): its box is part of the element's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PseudoId {
    Before,
    After,
    /// A list item's marker.
    Marker,
}

/// The computed styles of an element's pseudo-elements, stored with the
/// element's (Stylo's `EagerPseudoStyles`): one for each that a rule
/// matches, and a list item's `::marker`'s (the user-agent stylesheet's
/// rule always matches it). Only a `::before` or `::after` with `content`
/// generates a box, and a marker with something to show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PseudoStyles {
    pub before: Option<Arc<ComputedStyle>>,
    pub after: Option<Arc<ComputedStyle>>,
    pub marker: Option<Arc<ComputedStyle>>,
}

impl PseudoStyles {
    pub fn get(&self, pseudo: PseudoId) -> Option<&Arc<ComputedStyle>> {
        match pseudo {
            PseudoId::Before => self.before.as_ref(),
            PseudoId::After => self.after.as_ref(),
            PseudoId::Marker => self.marker.as_ref(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.before.is_none() && self.after.is_none() && self.marker.is_none()
    }
}

/// `content` (css-content 3): what a `::before`, `::after` or `::marker`
/// generates.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// `none` on `::before` and `::after`; a marker's text from its
    /// `list-style-type`; an element's own `content` doesn't replace it.
    Normal,
    None,
    Items(Arc<[ContentItem]>),
}

/// One item of `content`'s list.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentItem {
    String(Arc<str>),
    /// `attr(name)`: the element's attribute's value, which `style` puts
    /// in, so that a new value is a new style; serialized as the function.
    Attr {
        name: Arc<str>,
        value: Arc<str>,
    },
    /// `counter(name, style)`: the innermost counter of that name's value,
    /// which layout works out.
    Counter {
        name: Arc<str>,
        style: ListStyleType,
    },
    /// `counters(name, separator, style)`: the values of every counter of
    /// that name in the element's set, outermost first.
    Counters {
        name: Arc<str>,
        separator: Arc<str>,
        style: ListStyleType,
    },
}

/// `list-style-type` (css-lists 3), and the counter style of `counter()`
/// and `counters()`: the predefined counter styles machopo has
/// (css-counter-styles 3), `none`, and for a marker a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListStyleType {
    Disc,
    Circle,
    Square,
    DisclosureOpen,
    DisclosureClosed,
    Decimal,
    DecimalLeadingZero,
    LowerRoman,
    UpperRoman,
    LowerGreek,
    /// `lower-alpha` and `lower-latin` are the same style, each kept as
    /// written.
    LowerAlpha,
    LowerLatin,
    UpperAlpha,
    UpperLatin,
    None,
    /// A counter style machopo doesn't have, by its name as written (a
    /// predefined one, or one `@counter-style` would define): rendered as
    /// `decimal`, the fallback of one that isn't defined.
    Named(Arc<str>),
    /// The marker's text, as is (`list-style-type` only).
    String(Arc<str>),
}

/// `list-style-position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListStylePosition {
    /// Outside the list item's first line, before its start.
    Outside,
    /// The first inline box of the list item's first line.
    Inside,
}

/// One counter `counter-reset` instantiates (css-lists 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterReset {
    pub name: Arc<str>,
    /// `None` for a `reversed()` one without a value: layout works it out
    /// from the counter's increments.
    pub value: Option<i32>,
    pub reversed: bool,
}

/// One counter `counter-increment` or `counter-set` changes, by or to
/// `value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterChange {
    pub name: Arc<str>,
    pub value: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexWrap {
    Nowrap,
    Wrap,
    WrapReverse,
}

/// `justify-content` / `align-content` (css-align 3): how the free space
/// between and around items or lines is shared. `left` / `right` and the
/// `safe` / `unsafe` and baseline values are not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentAlignment {
    /// What the layout mode does by default: `flex-start` for
    /// `justify-content`, `stretch` for `align-content` in flex.
    Normal,
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// `align-items` / `align-self` (css-align 3): where an item goes in the
/// cross axis. `self-start` and `self-end` compute to `start` and `end`,
/// the same with no writing modes; `last baseline` is not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAlignment {
    /// `stretch` for flex items.
    Normal,
    Stretch,
    Start,
    End,
    /// `self-start` / `self-end`: the item's own start and end, the same
    /// as `start` and `end` while every box is horizontal and
    /// left-to-right.
    SelfStart,
    SelfEnd,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
}

/// `float` (CSS 2 § 9.5.1; `inline-start` and `inline-end` aren't
/// supported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Float {
    None,
    Left,
    Right,
}

/// `clear` (CSS 2 § 9.5.2; `inline-start` and `inline-end` aren't
/// supported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clear {
    None,
    Left,
    Right,
    Both,
}

/// `position` (css-position 3), the values laid out so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Static,
    /// In flow, then moved by `top` / `right` / `bottom` / `left`.
    Relative,
    /// Out of flow, laid out against its containing block (the nearest
    /// positioned ancestor's padding box, else the initial containing
    /// block).
    Absolute,
    /// Out of flow, laid out against the viewport of the frame it is in,
    /// and not moved when that viewport scrolls.
    Fixed,
    /// In flow, then moved as its nearest scrollport scrolls to stay
    /// `top` / `right` / `bottom` / `left` inside it, as far as its
    /// containing block lets it.
    Sticky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

/// A color property's computed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleColor {
    CurrentColor,
    Rgba(Color),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    /// `none`, but for table cells' border conflicts (which machopo doesn't
    /// resolve yet).
    Hidden,
    Solid,
    Dotted,
    Dashed,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    /// Whether the side is drawn at all.
    pub fn is_drawn(self) -> bool {
        !matches!(self, Self::None | Self::Hidden)
    }

    /// Whether it covers the whole side, as `solid` does: what a
    /// background under it can't show through (Blink's
    /// `BorderEdge::ObscuresBackgroundEdge`; not the gaps of `dotted`,
    /// `dashed` and `double`).
    pub fn is_filled(self) -> bool {
        matches!(
            self,
            Self::Solid | Self::Groove | Self::Ridge | Self::Inset | Self::Outset
        )
    }
}

/// `outline-style`: `auto` (the platform's focus ring) or a border style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineStyle {
    Auto,
    /// `none` and the border styles (not `hidden`, which an outline
    /// doesn't take).
    Line(BorderStyle),
}

impl OutlineStyle {
    /// Whether an outline is drawn at all.
    pub fn is_drawn(self) -> bool {
        match self {
            Self::Auto => true,
            Self::Line(style) => style.is_drawn(),
        }
    }
}

/// One `box-shadow` (Blink's `ShadowData`): its offsets, blur radius and
/// spread distance in px, its color, and whether it is `inset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: StyleColor,
    pub inset: bool,
}

/// One `filter` function, computed (Blink's `FilterOperation`): amounts
/// as numbers (1 for 100%), at least 0, at most 1 for `grayscale()`,
/// `invert()`, `opacity()` and `sepia()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterFunction {
    /// The standard deviation, in px.
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    /// No spread, never `inset`; its blur a radius, as `box-shadow`'s.
    DropShadow(BoxShadow),
    Grayscale(f32),
    /// Degrees.
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
}

/// `isolation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Isolation {
    #[default]
    Auto,
    Isolate,
}

/// `overflow-x` / `overflow-y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    #[default]
    Visible,
    /// Clipped, but not a scroll container (css-overflow 3): no scrolling
    /// at all, no block formatting context of its own, and a flex item
    /// keeps its automatic minimum size. Next to an axis that scrolls it
    /// computes to `hidden`.
    Clip,
    /// Clipped, and scrollable only programmatically (a `#fragment`).
    Hidden,
    /// Scrollable by the user when the content overflows.
    Auto,
    /// Scrollable by the user; without scrollbars, the same as `auto`.
    Scroll,
}

impl Overflow {
    /// Whether the user can scroll this axis (wheel, keys).
    pub fn user_scrollable(self) -> bool {
        matches!(self, Self::Auto | Self::Scroll)
    }
}

/// One function of `transform` (2D only; Blink's `TransformOperation`
/// kinds). Percentages in `Translate` are of the reference box
/// (`transform-box`), resolved when the box is known.
#[derive(Debug, Clone, PartialEq)]
pub enum TransformOperation {
    Translate(LengthPercentage, LengthPercentage),
    Scale(f32, f32),
    /// `rotate()` and `rotateZ()`, in degrees: kept as an angle, not a
    /// matrix, so that a turn interpolates as a turn.
    Rotate(f32),
    /// `skew()`: the x and the y angles in degrees.
    Skew(f32, f32),
    /// `skewX()` and `skewY()`, in degrees: kinds of their own, as each
    /// interpolates only with its own kind (Blink's `kSkewX`, `kSkewY`).
    SkewX(f32),
    SkewY(f32),
    /// `matrix(a, b, c, d, e, f)`.
    Matrix([f32; 6]),
    /// `from` `progress` of the way to `to`, two lists that interpolate
    /// as matrices but need the box for them (a percentage): kept until
    /// it is known (Blink's `InterpolatedTransformOperation`).
    Interpolated {
        from: Arc<[TransformOperation]>,
        to: Arc<[TransformOperation]>,
        progress: f32,
    },
}

impl TransformOperation {
    /// The function as a matrix, percentages against `basis` (the
    /// reference box's size).
    pub fn matrix(&self, basis: SizeF) -> Transform {
        match *self {
            Self::Translate(x, y) => {
                Transform::translate(x.resolve(basis.width), y.resolve(basis.height))
            }
            Self::Scale(x, y) => Transform::scale(x, y),
            Self::Rotate(degrees) => Transform::rotate(degrees),
            Self::Skew(x, y) => Transform::skew(x, y),
            Self::SkewX(x) => Transform::skew(x, 0.0),
            Self::SkewY(y) => Transform::skew(0.0, y),
            Self::Matrix(m) => Transform(m),
            Self::Interpolated {
                ref from,
                ref to,
                progress,
            } => {
                let (from, to) = (
                    transform_list_matrix(from, basis),
                    transform_list_matrix(to, basis),
                );
                // One that can't be inverted steps at the middle (css-
                // transforms 2, "Interpolation of Matrices").
                Transform::blend(&from, &to, progress.into()).unwrap_or(if progress < 0.5 {
                    from
                } else {
                    to
                })
            }
        }
    }
}

/// `transform`'s functions as one matrix, the first outermost,
/// percentages against `basis` (the reference box's size): identity for
/// none.
pub fn transform_list_matrix(operations: &[TransformOperation], basis: SizeF) -> Transform {
    operations
        .iter()
        .fold(Transform::IDENTITY, |matrix, operation| {
            matrix.then(&operation.matrix(basis))
        })
}

/// `transform-origin`'s computed value: the point in the reference box
/// the transform is about, and a depth that only 3D transforms, which
/// machopo doesn't have, would use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformOrigin {
    pub x: LengthPercentage,
    pub y: LengthPercentage,
    /// In px.
    pub z: f32,
}

impl TransformOrigin {
    /// `50% 50% 0`.
    pub const CENTER: Self = Self {
        x: LengthPercentage::Percent(50.0),
        y: LengthPercentage::Percent(50.0),
        z: 0.0,
    };
}

/// `tab-size`: tab stops every so many spaces (the space's advance with
/// `letter-spacing` and `word-spacing`), or every so many px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabSize {
    Spaces(f32),
    Px(f32),
}

/// `clip` (css-masking 1, deprecated, on absolutely positioned boxes): its
/// `rect()`'s edges in px from the border box's top left corner, top,
/// right, bottom, left, each `None` for `auto` (the border box's edge).
pub type ClipRect = [Option<f32>; 4];

/// A `<geometry-box>` (css-masking 1): the box a `clip-path` is laid on.
/// `fill-box`, `stroke-box` and `view-box` are an SVG element's; a CSS
/// box takes them as `border-box`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryBox {
    BorderBox,
    PaddingBox,
    ContentBox,
    MarginBox,
    FillBox,
    StrokeBox,
    ViewBox,
}

/// A `<shape-radius>` (css-shapes 1), computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeRadius {
    Length(LengthPercentage),
    ClosestSide,
    FarthestSide,
}

/// A `<basic-shape>` (css-shapes 1), computed: `rect()` and `xywh()` are
/// `inset()` by then (Blink's `BasicShapeInset`).
#[derive(Debug, Clone, PartialEq)]
pub enum BasicShape {
    /// Its insets from the reference box's top, right, bottom and left
    /// edges, and its corners' radii (horizontal, vertical) from the top
    /// left round.
    Inset {
        insets: [LengthPercentage; 4],
        radii: [(LengthPercentage, LengthPercentage); 4],
    },
    /// `None` for a center not given (the reference box's), which stays
    /// unwritten (Chrome's computed value).
    Circle {
        radius: ShapeRadius,
        center: Option<Position2D>,
    },
    Ellipse {
        radii: (ShapeRadius, ShapeRadius),
        center: Option<Position2D>,
    },
    /// Its points, and the radius `round` gives its corners (css-shapes
    /// 2), in px.
    Polygon {
        fill_rule: FillRule,
        round: f32,
        points: Arc<[(LengthPercentage, LengthPercentage)]>,
    },
    /// Its path data as written, in px of the reference box.
    Path {
        fill_rule: FillRule,
        segments: Arc<[PathSegment]>,
    },
}

/// `clip-path` (css-masking 1), computed: a shape on a box, a box's own
/// (rounded) shape, or a `<clipPath>` by `url()` (resolved, or a fragment
/// alone as written: an element of the same document).
#[derive(Debug, Clone, PartialEq)]
pub enum ClipPath {
    None,
    Shape(Arc<BasicShape>, GeometryBox),
    Box(GeometryBox),
    Url(Arc<str>),
}

/// `contain` (css-contain 2): the containment a box asks for (Blink's
/// `kContains*` bits). Which of it applies depends on the box: none to a
/// non-atomic inline box, nor size containment to a table or a cell, nor
/// any to a table row, row group or column (Blink's
/// `IsEligibleForSizeContainment` and `IsEligibleForPaintOrLayoutContainment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Contain(u8);

impl Contain {
    pub const NONE: Self = Self(0);
    pub const INLINE_SIZE: Self = Self(1);
    const BLOCK_SIZE: Self = Self(2);
    pub const SIZE: Self = Self(Self::INLINE_SIZE.0 | Self::BLOCK_SIZE.0);
    pub const LAYOUT: Self = Self(4);
    pub const STYLE: Self = Self(8);
    pub const PAINT: Self = Self(16);
    /// `content`: `layout style paint`.
    pub const CONTENT: Self = Self(Self::LAYOUT.0 | Self::STYLE.0 | Self::PAINT.0);
    /// `strict`: `size` and `content`.
    pub const STRICT: Self = Self(Self::SIZE.0 | Self::CONTENT.0);

    /// Whether it has all of `other`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// It and `other`.
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// `-webkit-box-orient`: a `-webkit-box`'s direction, its flex direction
/// `row` or `column`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxOrient {
    Horizontal,
    Vertical,
}

/// `-webkit-box-direction`: whether a `-webkit-box`'s items go the other
/// way (`row-reverse`, `column-reverse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxDirection {
    Normal,
    Reverse,
}

/// `-webkit-box-pack`: a `-webkit-box`'s `justify-content`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxPack {
    Start,
    Center,
    End,
    Justify,
}

/// `-webkit-box-align`: the alignment of all of a `-webkit-box`'s items in
/// the cross axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxAlign {
    Stretch,
    Start,
    Center,
    End,
    Baseline,
}

/// `object-fit`: how a replaced element's content is sized in its content
/// box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFit {
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

/// `transform-box`: the box `transform-origin` and `translate()`'s
/// percentages are of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformBox {
    ContentBox,
    BorderBox,
    FillBox,
    StrokeBox,
    ViewBox,
}

/// One entry of `font-family`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FontFamily {
    Generic(GenericFamily),
    Named(Box<str>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// `font-stretch`'s keywords (CSS Fonts 3), as a canvas's `fontStretch`
/// and the `font` shorthand take them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    #[default]
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl FontStretch {
    /// The width it asks a font for, as a percentage of normal.
    pub fn percentage(self) -> f32 {
        match self {
            Self::UltraCondensed => 50.0,
            Self::ExtraCondensed => 62.5,
            Self::Condensed => 75.0,
            Self::SemiCondensed => 87.5,
            Self::Normal => 100.0,
            Self::SemiExpanded => 112.5,
            Self::Expanded => 125.0,
            Self::ExtraExpanded => 150.0,
            Self::UltraExpanded => 200.0,
        }
    }
}

/// `font-variant-caps`, and a canvas's `fontVariantCaps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontVariantCaps {
    #[default]
    Normal,
    SmallCaps,
    AllSmallCaps,
    PetiteCaps,
    AllPetiteCaps,
    Unicase,
    TitlingCaps,
}

/// `font-variant-ligatures`: `none`, or what each kind of ligature is
/// set to (`None` its default); all `None` is `normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontVariantLigatures {
    pub none: bool,
    pub common: Option<bool>,
    pub discretionary: Option<bool>,
    pub historical: Option<bool>,
    pub contextual: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericFigure {
    Lining,
    Oldstyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericSpacing {
    Proportional,
    Tabular,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericFraction {
    Diagonal,
    Stacked,
}

/// `font-variant-numeric`: each part `None` (or `false`) for `normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontVariantNumeric {
    pub figure: Option<NumericFigure>,
    pub spacing: Option<NumericSpacing>,
    pub fraction: Option<NumericFraction>,
    pub ordinal: bool,
    pub slashed_zero: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EastAsianVariant {
    Jis78,
    Jis83,
    Jis90,
    Jis04,
    Simplified,
    Traditional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EastAsianWidth {
    Full,
    Proportional,
}

/// `font-variant-east-asian`: each part `None` (or `false`) for `normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FontVariantEastAsian {
    pub variant: Option<EastAsianVariant>,
    pub width: Option<EastAsianWidth>,
    pub ruby: bool,
}

/// The functions of `font-variant-alternates`, in css-fonts 4's order
/// (which WPT's parsing tests write them in; Chrome's computed value puts
/// `styleset()` and `character-variant()` last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlternatesFunction {
    Stylistic,
    Styleset,
    CharacterVariant,
    Swash,
    Ornaments,
    Annotation,
}

/// `font-variant-alternates`. Its functions name `@font-feature-values`
/// entries, which machopo hasn't: only `historical-forms` takes effect.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FontVariantAlternates {
    pub historical_forms: bool,
    /// Each function and its names, in [`AlternatesFunction`]'s order.
    pub functions: Arc<[(AlternatesFunction, AlternatesNames)]>,
}

/// The `@font-feature-values` names a `font-variant-alternates` function
/// takes.
pub type AlternatesNames = Arc<[Box<str>]>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontVariantPosition {
    #[default]
    Normal,
    Sub,
    Super,
}

/// `font-variant-emoji`: parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontVariantEmoji {
    #[default]
    Normal,
    Text,
    Emoji,
    Unicode,
}

/// One of `font-feature-settings`: an OpenType feature tag and its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFeatureSetting {
    pub tag: [u8; 4],
    pub value: u32,
}

/// One of `font-variation-settings`: an axis tag and its value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontVariationSetting {
    pub tag: [u8; 4],
    pub value: f32,
}

/// `font-size-adjust`'s font metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSizeAdjustMetric {
    ExHeight,
    CapHeight,
    ChWidth,
    IcWidth,
    IcHeight,
}

/// `font-size-adjust`: parsed only (machopo doesn't scale the font by it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSizeAdjust {
    None,
    /// The metric, and the value: `None` is `from-font`.
    Adjust(FontSizeAdjustMetric, Option<f32>),
}

/// `font-kerning`, and a canvas's `fontKerning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontKerning {
    #[default]
    Auto,
    Normal,
    None,
}

/// `text-rendering` (inherited), and a canvas's `textRendering`: neither
/// changes how machopo draws text yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextRendering {
    #[default]
    Auto,
    OptimizeSpeed,
    OptimizeLegibility,
    GeometricPrecision,
}

/// Which way text runs: `direction`'s values (machopo's pages have no
/// `direction` yet; a canvas's comes from its `dir` attributes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// A canvas's `textBaseline`: which line of the text is at the `y` it is
/// drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextBaseline {
    Top,
    Hanging,
    Middle,
    #[default]
    Alphabetic,
    Ideographic,
    Bottom,
}

/// What a canvas's text is laid out in: its font, which way it runs, its
/// spacings in px and its kerning.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasTextStyle {
    pub font: CanvasFont,
    pub direction: Direction,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub kerning: FontKerning,
}

/// A canvas's font as its 2D context computes it from `font` (Blink's
/// `FontDescription` for a canvas): the parts of the `font` shorthand
/// that pick and size a font, `line-height` left out.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasFont {
    pub family: Arc<[FontFamily]>,
    /// In px.
    pub size: f32,
    pub weight: f32,
    pub style: FontStyle,
    pub stretch: FontStretch,
    pub variant_caps: FontVariantCaps,
}

impl Default for CanvasFont {
    /// HTML's default, `10px sans-serif`.
    fn default() -> Self {
        Self {
            family: Arc::new([FontFamily::Generic(GenericFamily::SansSerif)]),
            size: 10.0,
            weight: 400.0,
            style: FontStyle::Normal,
            stretch: FontStretch::Normal,
            variant_caps: FontVariantCaps::Normal,
        }
    }
}

/// `line-height`, computed: a number stays a number (and inherits as
/// one); lengths and percentages become px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Normal,
    Number(f32),
    Px(f32),
}

impl LineHeight {
    /// A number's line height in px, at `font_size`: in the range a
    /// computed length is (a big number times a big font is past any
    /// float; Blink's `LayoutUnit` saturates it).
    pub fn number_px(number: f32, font_size: f32) -> f32 {
        clamp_css_length(number * font_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
}

/// `white-space-collapse` (`preserve-spaces` and `discard`, in the spec,
/// aren't in Chrome either).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpaceCollapse {
    Collapse,
    Preserve,
    PreserveBreaks,
    BreakSpaces,
}

impl WhiteSpaceCollapse {
    /// Whether spaces and tabs collapse.
    pub fn collapses_spaces(self) -> bool {
        matches!(self, Self::Collapse | Self::PreserveBreaks)
    }

    /// Whether newlines are kept as forced breaks.
    pub fn preserves_newlines(self) -> bool {
        !matches!(self, Self::Collapse)
    }
}

/// `text-wrap-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWrapMode {
    Wrap,
    Nowrap,
}

/// The `white-space` shorthand's keywords, each a `white-space-collapse`
/// and a `text-wrap-mode` (css-text 4); the pairs with no keyword are
/// written as the two longhands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Pre,
    Nowrap,
    PreWrap,
    PreLine,
    BreakSpaces,
}

impl WhiteSpace {
    pub fn longhands(self) -> (WhiteSpaceCollapse, TextWrapMode) {
        use TextWrapMode::{Nowrap, Wrap};
        use WhiteSpaceCollapse::{BreakSpaces, Collapse, Preserve, PreserveBreaks};
        match self {
            Self::Normal => (Collapse, Wrap),
            Self::Pre => (Preserve, Nowrap),
            Self::Nowrap => (Collapse, Nowrap),
            Self::PreWrap => (Preserve, Wrap),
            Self::PreLine => (PreserveBreaks, Wrap),
            Self::BreakSpaces => (BreakSpaces, Wrap),
        }
    }

    /// The keyword for this pair, if it has one.
    pub fn from_longhands(collapse: WhiteSpaceCollapse, wrap: TextWrapMode) -> Option<Self> {
        [
            Self::Normal,
            Self::Pre,
            Self::Nowrap,
            Self::PreWrap,
            Self::PreLine,
            Self::BreakSpaces,
        ]
        .into_iter()
        .find(|keyword| keyword.longhands() == (collapse, wrap))
    }
}

/// `vertical-align`, for an inline-level box (CSS 2's values).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalAlign {
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    Top,
    Bottom,
    /// The baseline raised by this much; a percentage of the box's own
    /// `line-height`.
    Length(LengthPercentage),
}

/// `text-overflow` (css-overflow 4; the `<string>` value and two values
/// aren't supported, as in Chrome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    Clip,
    Ellipsis,
}

/// `appearance` (css-ui 4), with the compat keywords Chrome keeps: whether
/// a form control looks native (`auto`, or the widget a keyword names) or
/// as CSS draws it (`none`). What it is used as also depends on the element
/// and on the author's styles (Blink's `EffectiveAppearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    None,
    Auto,
    Checkbox,
    Radio,
    Button,
    Listbox,
    Menulist,
    MenulistButton,
    Meter,
    ProgressBar,
    Searchfield,
    Textarea,
    Textfield,
    BaseSelect,
}

/// The native look a form control has, if any: what `appearance` comes to
/// for the element and the author's styles (Blink's `EffectiveAppearance`,
/// whose parts include ones no keyword names, as `push-button`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectiveAppearance {
    None,
    Checkbox,
    Radio,
    PushButton,
    /// `<input type=color>`'s.
    SquareButton,
    Listbox,
    Menulist,
    /// A menu list drawn by CSS, with the arrow.
    MenulistButton,
    Meter,
    ProgressBar,
    /// `<input type=range>`'s.
    SliderHorizontal,
    SearchField,
    TextField,
    TextArea,
    /// A search field's clear button (its UA shadow tree's).
    SearchFieldCancelButton,
    /// A number field's spin button (its UA shadow tree's).
    InnerSpinButton,
    /// A range's thumb (its UA shadow tree's).
    SliderThumbHorizontal,
}

/// `text-transform` (`full-width` and `full-size-kana` aren't supported).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Capitalize,
    Uppercase,
    Lowercase,
}

/// `word-break`; `break-word` is Chrome's legacy value, `normal` plus
/// `overflow-wrap: anywhere`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
    BreakWord,
}

/// `overflow-wrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowWrap {
    Normal,
    BreakWord,
    Anywhere,
}

/// `text-decoration-line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextDecorationLine {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    /// Kept and written back, never drawn (css-text-decor 3 lets a UA not
    /// blink; Chrome doesn't).
    pub blink: bool,
}

impl TextDecorationLine {
    pub const NONE: Self = Self {
        underline: false,
        overline: false,
        line_through: false,
        blink: false,
    };

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }
}

/// `text-decoration-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// `text-decoration-thickness` (Blink's `TextDecorationThickness`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TextDecorationThickness {
    /// A tenth of the font size.
    #[default]
    Auto,
    /// The font's underline thickness, else `auto`'s.
    FromFont,
    /// A percentage is of the font size.
    Length(LengthPercentage),
}

/// `text-underline-position`: `auto` is none of them; one of `from_font`
/// and `under`, one of `left` and `right` (for vertical text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextUnderlinePosition {
    pub from_font: bool,
    pub under: bool,
    pub left: bool,
    pub right: bool,
}

/// What an element with `text-decoration-line` draws on the text in it
/// (css-text-decor 3, "decorating box"; Blink's `AppliedTextDecoration`):
/// its lines in its style, color (`currentcolor` its own `color`) and
/// thickness, and its `text-underline-offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppliedTextDecoration {
    pub lines: TextDecorationLine,
    pub style: TextDecorationStyle,
    pub color: Color,
    pub thickness: TextDecorationThickness,
    /// `None` is `auto`; a percentage is of the font size.
    pub underline_offset: Option<LengthPercentage>,
}

/// One layer's `background-image` (css-backgrounds 3; Blink's
/// `StyleImage` on a `FillLayer`). An image is only its URL: the renderer
/// loads it, and paint looks it up by URL, so the style stays plain data.
#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundImage {
    None,
    Url(ImageUrl),
    Gradient(Arc<Gradient>),
    /// `image-set()`: every option, the one shown picked for the device
    /// scale factor where it is shown ([`BackgroundImage::shown`]), as
    /// Blink picks it once the style is resolved (`ElementStyleResources`)
    /// and keeps the set for `getComputedStyle` (`StyleImageSet`).
    Set(Arc<[ImageSetOption]>),
}

/// A `url()` image (Blink's `StyleFetchedImage`'s `CSSImageValue`): the
/// URL resolved against the stylesheet's (or the document's) base URL, as
/// written if it didn't resolve; and who it is fetched for, the referrer
/// of where it was parsed (the stylesheet's URL, or the document's),
/// which DevTools names as the request's initiator. Two of one URL are the
/// same image, whoever they are for.
#[derive(Debug, Clone)]
pub struct ImageUrl {
    pub url: Arc<str>,
    pub referrer: Option<Arc<str>>,
}

impl PartialEq for ImageUrl {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

impl From<&str> for ImageUrl {
    /// `url`, for no one in particular.
    fn from(url: &str) -> Self {
        Self {
            url: url.into(),
            referrer: None,
        }
    }
}

/// One option of an `image-set()`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageSetOption {
    /// A `Url` or a `Gradient`.
    pub image: BackgroundImage,
    /// In dppx: the image's natural size is its size divided by this.
    pub resolution: f32,
    /// `type()`'s MIME type, as written.
    pub mime_type: Option<Arc<str>>,
}

/// The MIME types of the images machopo decodes (`raster`'s formats), which
/// an `image-set()` option's `type()` may name (ASCII case-insensitive);
/// Chrome takes the types it decodes too (`IsSupportedImageMimeType`).
const SUPPORTED_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/svg+xml"];

impl BackgroundImage {
    /// The image shown at `device_scale_factor`, and its resolution in
    /// dppx: itself at 1 but for an `image-set()`, whose option is (as
    /// Blink's `CSSImageSetValue::GetBestOption`) the one of the lowest
    /// resolution at least `device_scale_factor` among those of a type machopo
    /// decodes, else the one of the highest, the first written of equals;
    /// none if each has an unsupported type, or the one picked is of 0dppx
    /// (which Chrome doesn't paint either).
    pub fn shown(&self, device_scale_factor: f32) -> Option<(&BackgroundImage, f32)> {
        let options = match self {
            Self::None => return None,
            Self::Set(options) => options,
            image => return Some((image, 1.0)),
        };
        let supported = options.iter().filter(|option| {
            option.mime_type.as_deref().is_none_or(|mime| {
                SUPPORTED_IMAGE_TYPES
                    .iter()
                    .any(|supported| mime.eq_ignore_ascii_case(supported))
            })
        });
        let mut best: Option<&ImageSetOption> = None;
        for option in supported {
            let better = match best {
                None => true,
                Some(best) => {
                    let (new, old) = (option.resolution, best.resolution);
                    if old >= device_scale_factor {
                        new >= device_scale_factor && new < old
                    } else {
                        new > old
                    }
                }
            };
            if better {
                best = Some(option);
            }
        }
        best.filter(|option| option.resolution > 0.0)
            .map(|option| (&option.image, option.resolution))
    }

    /// The URL loaded to show it at `device_scale_factor`, if it is one.
    pub fn shown_url(&self, device_scale_factor: f32) -> Option<&ImageUrl> {
        match self.shown(device_scale_factor)? {
            (Self::Url(url), _) => Some(url),
            _ => None,
        }
    }
}

/// `linear-gradient()` / `radial-gradient()` and their `repeating-`
/// forms, computed: lengths in px, colors as for a color property.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub repeating: bool,
    /// The color stops and hints, in order; at least two stops, and a
    /// hint only between two stops.
    pub items: Arc<[GradientItem]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    Linear(LinearDirection),
    Radial {
        shape: RadialShape,
        size: RadialSize,
        position: Position2D,
    },
}

/// Which way a linear gradient's line goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearDirection {
    /// Degrees clockwise from "up", as `<angle>` computes.
    Angle(f32),
    /// `to <side-or-corner>`: the horizontal side, the vertical side, at
    /// least one of them. `to bottom` is the default.
    To(Option<HorizontalSide>, Option<VerticalSide>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalSide {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadialShape {
    Circle,
    Ellipse,
}

/// How big a radial gradient's ending shape is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadialSize {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    /// A circle's radius.
    Circle(f32),
    /// An ellipse's two radii.
    Ellipse(LengthPercentage, LengthPercentage),
}

/// One color stop (its position `None` where it is to be spread out
/// between its neighbours), or a color hint: where between two stops the
/// color is halfway.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientItem {
    Stop(StyleColor, Option<LengthPercentage>),
    Hint(LengthPercentage),
}

/// A `<position>` component, computed (css-values 4): the offset from the
/// start (left or top) side, or from the end side for a length after
/// `right` / `bottom` (`right 10px` is `calc(100% - 10px)`, which has no
/// other form without `calc()`). Keywords and percentages from the end
/// side come to percentages from the start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionComponent {
    pub from_end: bool,
    pub offset: LengthPercentage,
}

impl PositionComponent {
    pub const START: Self = Self {
        from_end: false,
        offset: LengthPercentage::Percent(0.0),
    };
    pub const CENTER: Self = Self {
        from_end: false,
        offset: LengthPercentage::Percent(50.0),
    };

    /// Where in `area` px the component puts its point (for
    /// `background-position`, `area` is the room left beside the image).
    pub fn resolve(self, area: f32) -> f32 {
        let offset = self.offset.resolve(area);
        if self.from_end { area - offset } else { offset }
    }
}

/// A `<position>` (a radial gradient's center), computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position2D {
    pub x: PositionComponent,
    pub y: PositionComponent,
}

impl Position2D {
    pub const CENTER: Self = Self {
        x: PositionComponent::CENTER,
        y: PositionComponent::CENTER,
    };
}

/// `background-size` for one layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundSize {
    Cover,
    Contain,
    /// Width and height; `auto` is from the image.
    Explicit(LengthPercentageOrAuto, LengthPercentageOrAuto),
}

impl BackgroundSize {
    pub const AUTO: Self =
        Self::Explicit(LengthPercentageOrAuto::Auto, LengthPercentageOrAuto::Auto);
}

/// `background-repeat` for one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundRepeat {
    Repeat,
    /// As many whole images as fit, the room left shared between them.
    Space,
    /// As many as fit when each is scaled to fill the area.
    Round,
    NoRepeat,
}

/// `background-attachment`. Only `scroll` is painted: `fixed` and `local`
/// are painted as it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundAttachment {
    Scroll,
    Fixed,
    Local,
}

/// `background-clip` / `background-origin`: one of the box's boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundBox {
    BorderBox,
    PaddingBox,
    ContentBox,
}

/// One background layer, the longhands' values for it put together
/// (Blink's `FillLayer`).
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundLayer {
    pub image: BackgroundImage,
    pub position_x: PositionComponent,
    pub position_y: PositionComponent,
    pub size: BackgroundSize,
    pub repeat: (BackgroundRepeat, BackgroundRepeat),
    pub attachment: BackgroundAttachment,
    pub clip: BackgroundBox,
    pub origin: BackgroundBox,
}

/// One corner's `border-*-radius`: its horizontal and vertical radii,
/// percentages of the border box's width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadius {
    pub x: LengthPercentage,
    pub y: LengthPercentage,
}

impl CornerRadius {
    pub const ZERO: Self = Self {
        x: LengthPercentage::ZERO,
        y: LengthPercentage::ZERO,
    };
}

/// A value per corner, in `border-radius` shorthand order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}

impl<T: Copy> Corners<T> {
    pub const fn all(value: T) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }
}

/// A number or a percentage, as written: `border-image-slice`'s sides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumberOrPercentage {
    Number(f32),
    Percent(f32),
}

/// `border-image-slice`: each side's offset into the image, non-negative;
/// `fill` draws the image's middle too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderImageSlice {
    pub sides: Sides<NumberOrPercentage>,
    pub fill: bool,
}

/// `border-image-width`'s side: a multiple of the border's width, a length
/// or percentage, or the image's own size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderImageWidth {
    Number(f32),
    LengthPercentage(LengthPercentage),
    Auto,
}

/// `border-image-outset`'s side: a multiple of the border's width, or px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderImageOutset {
    Number(f32),
    Px(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderImageRepeat {
    Stretch,
    Repeat,
    Round,
    Space,
}

/// The `border-image-*` longhands. machopo doesn't draw a border image (the
/// border is drawn as its style says) nor loads one: parsed only.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderImage {
    pub source: BackgroundImage,
    pub slice: BorderImageSlice,
    /// Non-negative.
    pub width: Sides<BorderImageWidth>,
    /// Non-negative.
    pub outset: Sides<BorderImageOutset>,
    /// Horizontal, vertical.
    pub repeat: (BorderImageRepeat, BorderImageRepeat),
}

impl BorderImage {
    pub fn initial() -> Self {
        Self {
            source: BackgroundImage::None,
            slice: BorderImageSlice {
                sides: Sides::all(NumberOrPercentage::Percent(100.0)),
                fill: false,
            },
            width: Sides::all(BorderImageWidth::Number(1.0)),
            outset: Sides::all(BorderImageOutset::Number(0.0)),
            repeat: (BorderImageRepeat::Stretch, BorderImageRepeat::Stretch),
        }
    }
}

/// A value per physical side, in `margin` / `padding` shorthand order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T> Sides<T> {
    /// Top, right, bottom, left.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        [
            &mut self.top,
            &mut self.right,
            &mut self.bottom,
            &mut self.left,
        ]
        .into_iter()
    }
}

impl<T: Copy> Sides<T> {
    pub const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// `visibility` (inherited): a `hidden` or `collapse` box is laid out but
/// not painted or hit, though its descendants may be `visible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Hidden,
    /// A table row, row group, column or column group takes no room
    /// (layout collapses it); on anything else, as `hidden`.
    Collapse,
}

/// `-webkit-text-security` (inherited): what each character of the text is
/// drawn as, a password field's (Blink's `ETextSecurity`): itself, or a
/// disc, a circle or a square.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSecurity {
    None,
    Disc,
    Circle,
    Square,
}

impl TextSecurity {
    /// The character drawn for each of the text's (Blink's
    /// `LayoutText::SecureText`): `•`, `◦` or `■`.
    pub fn mask(self) -> Option<char> {
        match self {
            Self::None => None,
            Self::Disc => Some('\u{2022}'),
            Self::Circle => Some('\u{25E6}'),
            Self::Square => Some('\u{25A0}'),
        }
    }
}

/// `cursor`'s keyword (Blink's `ECursor`): what the pointer looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorKeyword {
    Auto,
    Default,
    None,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    EResize,
    NResize,
    NeResize,
    NwResize,
    SResize,
    SeResize,
    SwResize,
    WResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    AllScroll,
    ZoomIn,
    ZoomOut,
}

/// An image in `cursor`'s list: a `Url` or an `image-set()` of `Url`s,
/// each resolved (as written when it is empty or a fragment, as Chrome
/// has it), and its hot spot (Chrome keeps whole numbers).
#[derive(Debug, Clone, PartialEq)]
pub struct CursorImage {
    pub image: BackgroundImage,
    pub hot_spot: Option<(i32, i32)>,
}

/// `cursor` (inherited; Blink's `CursorList` and `ECursor`). machopo loads no
/// cursor images: the keyword after them is the one shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    pub images: Arc<[CursorImage]>,
    pub keyword: CursorKeyword,
}

impl Cursor {
    pub fn initial() -> Self {
        Self {
            images: Arc::new([]),
            keyword: CursorKeyword::Auto,
        }
    }
}

/// `user-select` (inherited, as in Chrome; css-ui 4 has it not inherited,
/// with `auto` looking at the parent's): what the user can select. machopo has
/// no selection of the page's text yet: parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSelect {
    Auto,
    Text,
    None,
    All,
}

/// `-webkit-user-modify` (inherited): whether the user can edit the
/// element's contents. machopo edits only form controls: parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserModify {
    ReadOnly,
    ReadWrite,
    ReadWritePlaintextOnly,
}

/// `hyphens` (inherited): where a word may break with a hyphen. machopo breaks
/// only at soft hyphens (U+00AD), with a hyphen, unless it is `none`:
/// `auto` finds no other places, as there are no hyphenation dictionaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hyphens {
    None,
    Manual,
    Auto,
}

/// `forced-color-adjust` (inherited). machopo has no forced colors mode:
/// parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedColorAdjust {
    Auto,
    None,
    PreserveParentColor,
}

/// `text-size-adjust` (inherited): how a mobile browser may enlarge the
/// text. machopo doesn't: parsed only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextSizeAdjust {
    Auto,
    /// Non-negative; `none` is 100% (Chrome's computed value).
    Percent(f32),
}

/// `-webkit-font-smoothing` (inherited): how glyphs are antialiased, on
/// macOS in Chrome. Parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSmoothing {
    Auto,
    None,
    Antialiased,
    SubpixelAntialiased,
}

/// `resize`: which way the user can resize the element. machopo draws no
/// resizer: parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resize {
    None,
    Both,
    Horizontal,
    Vertical,
    Block,
    Inline,
    /// Chrome's `auto`: `both` where text areas can be resized, which they
    /// can in Chrome; its computed value reads back as `both`.
    Auto,
}

/// `scroll-behavior`: whether a script's scroll is smooth. machopo scrolls
/// instantly: parsed only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBehavior {
    Auto,
    Smooth,
}

/// `overflow-anchor` (css-scroll-anchoring): whether the element and what
/// is in it (but for another scroll container's content) may be a scroll
/// anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowAnchor {
    Auto,
    None,
}

/// `pointer-events` (inherited): what of an element hit testing finds. An
/// HTML box goes by `none` only, every other value as `auto`; an SVG
/// shape by which of its fill and stroke count (`auto` is
/// `VisiblePainted` there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEvents {
    Auto,
    None,
    VisiblePainted,
    VisibleFill,
    VisibleStroke,
    Visible,
    Painted,
    Fill,
    Stroke,
    All,
    BoundingBox,
}

/// `border-collapse` (inherited): whether a table's cells have borders of
/// their own, `border-spacing` apart, or share them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderCollapse {
    Separate,
    Collapse,
}

/// `border-spacing` (inherited): px between a separated table's cells,
/// and between them and the table's edges. Non-negative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSpacing {
    pub horizontal: f32,
    pub vertical: f32,
}

impl BorderSpacing {
    pub const ZERO: Self = Self {
        horizontal: 0.0,
        vertical: 0.0,
    };
}

/// `table-layout`: whether a table's columns are sized by every cell's
/// content, or by the first row's widths (`fixed`, when the table's
/// `width` isn't `auto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableLayout {
    Auto,
    Fixed,
}

/// `caption-side` (inherited): which side of a table's grid its captions
/// go on. Chrome has `top` and `bottom` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionSide {
    Top,
    Bottom,
}

/// `empty-cells` (inherited): whether a separated table's cell with no
/// content paints its background and borders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyCells {
    Show,
    Hide,
}

/// `fill` and `stroke`: what an SVG shape is painted with (Blink's
/// `SVGPaint`).
#[derive(Debug, Clone, PartialEq)]
pub enum SvgPaint {
    None,
    Color(StyleColor),
    /// A paint server (a gradient, say) by URL, resolved against the
    /// stylesheet's (or the document's) base URL unless it is a fragment
    /// alone, and what to paint if it isn't one.
    Url(Arc<str>, SvgPaintFallback),
}

/// What a `url()` paint falls back to: written as `none` or a color, or
/// nothing, which is `none` for a URL that isn't to a paint server
/// (SVG 2, "Specifying paint").
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgPaintFallback {
    Absent,
    None,
    Color(StyleColor),
}

/// `text-anchor`: where a text chunk is placed against its start
/// position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// `stroke-linecap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeLinecap {
    Butt,
    Round,
    Square,
}

/// `stroke-linejoin` (Chrome has none of SVG 2's `miter-clip` and `arcs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeLinejoin {
    Miter,
    Round,
    Bevel,
}

/// One command of `d`'s path data, as written: relative commands keep
/// their offsets, absolute ones their points (`svgtypes`' segments).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    MoveTo {
        abs: bool,
        x: f32,
        y: f32,
    },
    LineTo {
        abs: bool,
        x: f32,
        y: f32,
    },
    HorizontalLineTo {
        abs: bool,
        x: f32,
    },
    VerticalLineTo {
        abs: bool,
        y: f32,
    },
    CurveTo {
        abs: bool,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x: f32,
        y: f32,
    },
    SmoothCurveTo {
        abs: bool,
        x2: f32,
        y2: f32,
        x: f32,
        y: f32,
    },
    Quadratic {
        abs: bool,
        x1: f32,
        y1: f32,
        x: f32,
        y: f32,
    },
    SmoothQuadratic {
        abs: bool,
        x: f32,
        y: f32,
    },
    EllipticalArc {
        abs: bool,
        rx: f32,
        ry: f32,
        x_axis_rotation: f32,
        large_arc: bool,
        sweep: bool,
        x: f32,
        y: f32,
    },
    ClosePath,
}

/// The SVG properties (Blink's `SVGComputedStyle`): painting, which is
/// inherited, and the geometry of shapes, which isn't.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgStyle {
    pub fill: SvgPaint,
    /// 0 to 1.
    pub fill_opacity: f32,
    pub fill_rule: FillRule,
    /// How a `<clipPath>`'s shape covers (css-masking 1; inherited).
    pub clip_rule: FillRule,
    pub stroke: SvgPaint,
    /// 0 to 1.
    pub stroke_opacity: f32,
    /// Non-negative; a percentage is of the viewport's normalized
    /// diagonal.
    pub stroke_width: LengthPercentage,
    pub stroke_linecap: StrokeLinecap,
    pub stroke_linejoin: StrokeLinejoin,
    /// Non-negative.
    pub stroke_miterlimit: f32,
    /// Non-negative; empty for `none`.
    pub stroke_dasharray: Arc<[LengthPercentage]>,
    pub stroke_dashoffset: LengthPercentage,
    /// SVG's text (inherited).
    pub text_anchor: TextAnchor,
    /// A gradient's `<stop>`'s color and opacity (not inherited).
    pub stop_color: StyleColor,
    /// 0 to 1.
    pub stop_opacity: f32,
    pub cx: LengthPercentage,
    pub cy: LengthPercentage,
    /// Non-negative.
    pub r: LengthPercentage,
    /// Non-negative.
    pub rx: LengthPercentageOrAuto,
    pub ry: LengthPercentageOrAuto,
    pub x: LengthPercentage,
    pub y: LengthPercentage,
    /// The path data with every command absolute (Blink's computed `d`);
    /// `None` for `none`.
    pub d: Option<Arc<[PathSegment]>>,
}

impl SvgStyle {
    pub fn initial() -> Self {
        Self {
            fill: SvgPaint::Color(StyleColor::Rgba(Color::BLACK)),
            fill_opacity: 1.0,
            fill_rule: FillRule::NonZero,
            clip_rule: FillRule::NonZero,
            stroke: SvgPaint::None,
            stroke_opacity: 1.0,
            stroke_width: LengthPercentage::Px(1.0),
            stroke_linecap: StrokeLinecap::Butt,
            stroke_linejoin: StrokeLinejoin::Miter,
            stroke_miterlimit: 4.0,
            stroke_dasharray: Arc::new([]),
            stroke_dashoffset: LengthPercentage::ZERO,
            text_anchor: TextAnchor::Start,
            stop_color: StyleColor::Rgba(Color::BLACK),
            stop_opacity: 1.0,
            cx: LengthPercentage::ZERO,
            cy: LengthPercentage::ZERO,
            r: LengthPercentage::ZERO,
            rx: LengthPercentageOrAuto::Auto,
            ry: LengthPercentageOrAuto::Auto,
            x: LengthPercentage::ZERO,
            y: LengthPercentage::ZERO,
            d: None,
        }
    }

    /// The inherited properties of `parent`, the geometry initial.
    #[allow(clippy::clone_on_copy)]
    pub fn inherit(parent: &Self) -> Self {
        macro_rules! from_parent {
            ($($field:ident),*) => {
                Self {
                    $($field: parent.$field.clone(),)*
                    ..Self::initial()
                }
            };
        }
        svg_inherited!(from_parent)
    }

    /// Whether the inherited properties are the same in both.
    pub fn same_inherited(&self, other: &Self) -> bool {
        macro_rules! same {
            ($($field:ident),*) => { true $(&& self.$field == other.$field)* };
        }
        svg_inherited!(same)
    }
}

/// The inherited properties of `SvgStyle`: one list, so that `inherit` and
/// `same_inherited` can't disagree.
macro_rules! svg_inherited {
    ($then:ident) => {
        $then!(
            fill,
            fill_opacity,
            fill_rule,
            clip_rule,
            stroke,
            stroke_opacity,
            stroke_width,
            stroke_linecap,
            stroke_linejoin,
            stroke_miterlimit,
            stroke_dasharray,
            stroke_dashoffset,
            text_anchor
        )
    };
}
use svg_inherited;

/// The inherited properties of `ComputedStyle` (`font` and
/// `rare_inherited`, which are shared whole, and `svg`'s apart:
/// `svg_inherited`): one list, so that `inherit` and `same_inherited` can't
/// disagree.
macro_rules! inherited {
    ($then:ident) => {
        $then!(
            color,
            line_height,
            text_align,
            white_space_collapse,
            text_wrap_mode,
            text_transform,
            list_style_position,
            visibility,
            pointer_events,
            inside_blocking_wheel_event_handler,
            border_collapse,
            caption_side,
            empty_cells,
            variables
        )
    };
}

/// `ComputedStyle`'s non-inherited properties most elements leave
/// initial, in an `Arc` it shares (`ComputedStyle::rare_mut` to set one).
#[derive(Debug, Clone, PartialEq)]
pub struct RareNonInherited {
    /// The `animation-*` longhands.
    pub animations: Animations,
    /// Its `::selection`'s custom properties, if its own `::selection`
    /// rules declare some: its own, then its declarations'. A highlight's
    /// custom properties are its originating element's and its own, never
    /// its parent highlight's (css-pseudo 4, WPT's
    /// `highlight-cascade-009.html`), so they aren't in the
    /// `HighlightStyle` the children share. `None`: its own.
    pub highlight_variables: Option<StyleVariables>,
    pub grid_template_columns: TrackList,
    pub grid_template_rows: TrackList,
    /// `None` for `none`.
    pub grid_template_areas: Option<Arc<GridTemplateAreas>>,
    /// The implicit tracks' sizes, repeated.
    pub grid_auto_columns: Arc<[TrackSize]>,
    pub grid_auto_rows: Arc<[TrackSize]>,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_row_start: GridLine,
    pub grid_row_end: GridLine,
    pub grid_column_start: GridLine,
    pub grid_column_end: GridLine,
    /// `border-top-left-radius` and the others.
    pub border_radius: Corners<CornerRadius>,
    /// Not inherited, as the other two; empty is `none`.
    pub counter_reset: Arc<[CounterReset]>,
    pub counter_increment: Arc<[CounterChange]>,
    pub counter_set: Arc<[CounterChange]>,
    /// Only `::before`'s, `::after`'s and `::marker`'s generate anything.
    pub content: Content,
    pub transform_origin: TransformOrigin,
    pub transform_box: TransformBox,
    pub object_fit: ObjectFit,
    pub object_position: Position2D,
    /// The `-webkit-box` longhands: they apply to a `-webkit-box` and its
    /// items only.
    pub box_orient: BoxOrient,
    pub box_direction: BoxDirection,
    pub box_pack: BoxPack,
    pub box_align: BoxAlign,
    pub box_flex: f32,
    /// At least 1.
    pub box_ordinal_group: u32,
    /// `-webkit-line-clamp`: `None` is `none`, else at least 1.
    pub line_clamp: Option<u32>,
    pub contain: Contain,
    /// `clip`: `None` is `auto`.
    pub clip: Option<ClipRect>,
    pub clip_path: ClipPath,
    pub text_decoration_style: TextDecorationStyle,
    pub text_decoration_color: StyleColor,
    pub text_decoration_thickness: TextDecorationThickness,
    /// `filter`, applied in order; empty for `none`.
    pub filter: Arc<[FilterFunction]>,
    pub isolation: Isolation,
    /// Its `display` was `-webkit-box` or `-webkit-inline-box`, made
    /// `flow-root` or `inline-block` by a vertical `-webkit-line-clamp`
    /// (Blink's `IsSpecifiedDisplayWebkitBox`): not a property's, set by
    /// the style's adjustment.
    pub specified_display_webkit_box: bool,
    pub resize: Resize,
    pub scroll_behavior: ScrollBehavior,
    pub overflow_anchor: OverflowAnchor,
    pub border_image: BorderImage,
}

/// `ComputedStyle`'s font properties, in an `Arc` it shares with its parent
/// until one is set (`ComputedStyle::font_mut`; Blink's `FontDescription`):
/// the `font` shorthand sets them together.
#[derive(Debug, Clone, PartialEq)]
pub struct FontDescription {
    pub family: Arc<[FontFamily]>,
    /// px.
    pub size: f32,
    /// 1 to 1000; `normal` is 400, `bold` 700.
    pub weight: f32,
    pub style: FontStyle,
    /// `font-stretch`, as a percentage of normal: not negative.
    pub stretch: f32,
    pub variant_caps: FontVariantCaps,
    pub variant_ligatures: FontVariantLigatures,
    pub variant_numeric: FontVariantNumeric,
    pub variant_east_asian: FontVariantEastAsian,
    pub variant_alternates: FontVariantAlternates,
    pub variant_position: FontVariantPosition,
    pub variant_emoji: FontVariantEmoji,
    /// Sorted by tag, one per tag (the last written); empty is `normal`.
    pub feature_settings: Arc<[FontFeatureSetting]>,
    /// Sorted by tag, one per tag (the last written); empty is `normal`.
    pub variation_settings: Arc<[FontVariationSetting]>,
    pub kerning: FontKerning,
    /// `font-optical-sizing: auto`.
    pub optical_sizing: bool,
    pub size_adjust: FontSizeAdjust,
    /// `font-language-override`: `None` is `normal`.
    pub language_override: Option<Arc<str>>,
    /// `font-synthesis-weight`, `-style` and `-small-caps`: whether a bold,
    /// an italic or small caps may be synthesized (`auto`).
    pub synthesis_weight: bool,
    pub synthesis_style: bool,
    pub synthesis_small_caps: bool,
    pub text_rendering: TextRendering,
    /// `-webkit-font-smoothing`.
    pub smoothing: FontSmoothing,
}

/// `ComputedStyle`'s inherited properties most elements leave as their
/// parent's, in an `Arc` it shares (`ComputedStyle::rare_inherited_mut` to
/// set one): inheriting them is one reference count.
#[derive(Debug, Clone, PartialEq)]
pub struct RareInherited {
    pub list_style_type: ListStyleType,
    /// px; `None` is `normal`, which 0 computes to as well (Chrome).
    pub letter_spacing: Option<f32>,
    /// px; `normal` is 0, as Chrome computes it.
    pub word_spacing: f32,
    pub tab_size: TabSize,
    pub text_indent: LengthPercentage,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    /// The decorations drawn on this element's text: its ancestors', then
    /// its own if it has a `text-decoration-line` (Blink's
    /// `AppliedTextDecorations`).
    pub applied_text_decorations: Arc<[AppliedTextDecoration]>,
    /// `text-shadow`, the first on top (no spread, never `inset`); empty
    /// for `none`.
    pub text_shadow: Arc<[BoxShadow]>,
    /// `-webkit-text-fill-color`: what the text is filled with, `color`
    /// unless set.
    pub text_fill_color: StyleColor,
    /// `None` is `auto`.
    pub text_underline_offset: Option<LengthPercentage>,
    pub text_underline_position: TextUnderlinePosition,
    pub text_security: TextSecurity,
    pub border_spacing: BorderSpacing,
    pub user_select: UserSelect,
    pub user_modify: UserModify,
    pub hyphens: Hyphens,
    pub forced_color_adjust: ForcedColorAdjust,
    pub tap_highlight_color: StyleColor,
    pub text_size_adjust: TextSizeAdjust,
    pub cursor: Cursor,
    pub caret_color: CaretColor,
    /// Its `::selection`'s style ([`HighlightStyle`]): `None` while no
    /// `::selection` rule matches it or an ancestor.
    pub highlight: Option<Arc<HighlightStyle>>,
}

/// `caret-color`'s computed value: `auto` is `currentcolor` (Chrome's; the
/// spec lets the UA pick one that contrasts), which `getComputedStyle`
/// reads back as the color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretColor {
    Auto,
    Color(StyleColor),
}

impl CaretColor {
    /// The color the caret is drawn in, for an element whose `color` is
    /// `color`.
    pub fn resolve(self, color: Color) -> Color {
        match self {
            Self::Auto | Self::Color(StyleColor::CurrentColor) => color,
            Self::Color(StyleColor::Rgba(rgba)) => rgba,
        }
    }
}

/// An element's `::selection` style (css-pseudo 4's highlight
/// pseudo-elements; Blink's `HighlightData` keeps a whole style): the
/// properties that apply to a highlight, each from the element's own
/// `::selection` rules or else from its parent's highlight style, as all of
/// them inherit there (highlight inheritance), and the initial values at the
/// root. Kept on the element's style and inherited with it
/// (`RareInherited::highlight`), so an element no rule of its own matches
/// shares its parent's.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightStyle {
    /// `currentcolor` is the originating element's `color`: kept as the
    /// keyword, which is what inherits (Chrome 152).
    pub color: StyleColor,
    pub background_color: StyleColor,
    pub text_decoration_line: TextDecorationLine,
    pub text_decoration_style: TextDecorationStyle,
    pub text_decoration_color: StyleColor,
    pub text_decoration_thickness: TextDecorationThickness,
    pub text_underline_offset: Option<LengthPercentage>,
    pub text_shadow: Arc<[BoxShadow]>,
    pub fill: SvgPaint,
    pub stroke: SvgPaint,
    pub stroke_width: LengthPercentage,
    /// Whether a rule for it here or for an ancestor's declared `color` or
    /// `background-color` (Blink's `HasAuthorHighlightColors`): without,
    /// it is painted in the default colors, both (css-pseudo 4's paired
    /// cascade; Chrome 152: a rule with only a decoration keeps them).
    pub author_colors: bool,
}

impl HighlightStyle {
    /// The highlight style of the root: the initial values, with `color`
    /// the originating element's.
    pub fn initial() -> Self {
        Self {
            color: StyleColor::CurrentColor,
            ..Self::of(&ComputedStyle::initial())
        }
    }

    /// `style`'s values of the highlight's properties, its `color` as
    /// computed.
    pub fn of(style: &ComputedStyle) -> Self {
        Self {
            color: StyleColor::Rgba(style.color),
            background_color: style.background_color,
            text_decoration_line: style.text_decoration_line,
            text_decoration_style: style.rare.text_decoration_style,
            text_decoration_color: style.rare.text_decoration_color,
            text_decoration_thickness: style.rare.text_decoration_thickness,
            text_underline_offset: style.rare_inherited.text_underline_offset,
            text_shadow: style.rare_inherited.text_shadow.clone(),
            fill: style.svg.fill.clone(),
            stroke: style.svg.stroke.clone(),
            stroke_width: style.svg.stroke_width,
            author_colors: false,
        }
    }

    /// `style`, the originating element's, with this highlight's values:
    /// what `getComputedStyle(element, '::selection')` reads (Chrome's other
    /// `currentcolor`s are this `color`, as they are here).
    pub fn apply_to(&self, style: &mut ComputedStyle) {
        if let StyleColor::Rgba(color) = self.color {
            style.color = color;
        }
        style.background_color = self.background_color;
        style.text_decoration_line = self.text_decoration_line;
        let rare = style.rare_mut();
        rare.text_decoration_style = self.text_decoration_style;
        rare.text_decoration_color = self.text_decoration_color;
        rare.text_decoration_thickness = self.text_decoration_thickness;
        let rare_inherited = style.rare_inherited_mut();
        rare_inherited.text_underline_offset = self.text_underline_offset;
        rare_inherited.text_shadow = self.text_shadow.clone();
        let svg = style.svg_mut();
        svg.fill = self.fill.clone();
        svg.stroke = self.stroke.clone();
        svg.stroke_width = self.stroke_width;
    }
}

/// `ComputedStyle`'s background layers' lists, in an `Arc` it shares
/// (`ComputedStyle::backgrounds_mut` to set one).
#[derive(Debug, Clone, PartialEq)]
pub struct Backgrounds {
    /// The background longhands' lists, each as declared (at least one
    /// item): there are as many layers as images, the other lists
    /// repeated or cut to match ([`Self::background_layers`]).
    pub background_image: Arc<[BackgroundImage]>,
    pub background_position_x: Arc<[PositionComponent]>,
    pub background_position_y: Arc<[PositionComponent]>,
    pub background_size: Arc<[BackgroundSize]>,
    pub background_repeat: Arc<[(BackgroundRepeat, BackgroundRepeat)]>,
    pub background_attachment: Arc<[BackgroundAttachment]>,
    pub background_clip: Arc<[BackgroundBox]>,
    pub background_origin: Arc<[BackgroundBox]>,
}

/// The computed style of one element.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    /// `display` before blockification (Blink's `OriginalDisplay`): an
    /// out-of-flow box's static position is where it would have been as
    /// the inline-level or block-level box it was.
    pub original_display: Display,
    pub box_sizing: BoxSizing,
    pub width: Size,
    pub height: Size,
    pub min_width: LengthPercentageOrAuto,
    pub min_height: LengthPercentageOrAuto,
    pub max_width: LengthPercentageOrNone,
    pub max_height: LengthPercentageOrNone,
    pub margin: Sides<LengthPercentageOrAuto>,
    pub position: Position,
    /// `none` for an out-of-flow box (CSS 2 § 9.7).
    pub float: Float,
    pub clear: Clear,
    /// `top`, `right`, `bottom` and `left`.
    pub inset: Sides<LengthPercentageOrAuto>,
    pub padding: Sides<LengthPercentage>,
    /// 0 on every side whose style is `none`.
    pub border_width: Sides<f32>,
    pub border_style: Sides<BorderStyle>,
    pub border_color: Sides<StyleColor>,
    /// `box-shadow`, the first on top; empty for `none`.
    pub box_shadow: Arc<[BoxShadow]>,
    pub background_color: StyleColor,
    pub text_decoration_line: TextDecorationLine,
    /// 0 to 1.
    pub opacity: f32,
    /// Neither is `visible` unless both are: `visible` next to one that
    /// isn't computes to `auto` (css-overflow).
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// Empty for `none`.
    pub transform: Arc<[TransformOperation]>,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    /// Non-negative.
    pub flex_grow: f32,
    /// Non-negative.
    pub flex_shrink: f32,
    pub flex_basis: FlexBasis,
    /// Where a flex item goes among its siblings: the container lays them
    /// out, and paints them, in `order`-modified document order.
    pub order: i32,
    pub justify_content: ContentAlignment,
    pub align_content: ContentAlignment,
    pub align_items: ItemAlignment,
    /// `None` is `auto`: the parent's `align-items`.
    pub align_self: Option<ItemAlignment>,
    /// Non-negative; `normal` is 0, which it is for flex containers.
    pub row_gap: LengthPercentage,
    pub column_gap: LengthPercentage,
    /// `legacy`, the initial value, is `normal` with no inheritance of
    /// `left` / `right` / `center`, which aren't supported.
    pub justify_items: ItemAlignment,
    /// `None` is `auto`: the parent's `justify-items`.
    pub justify_self: Option<ItemAlignment>,
    /// `None` is `auto`.
    pub z_index: Option<i32>,
    /// `z_index` where it applies, `None` elsewhere (Blink's
    /// `EffectiveZIndex`): on positioned boxes, and on flex and grid
    /// items. Style sets it, knowing the parent.
    pub effective_z_index: Option<i32>,
    pub vertical_align: VerticalAlign,
    pub table_layout: TableLayout,
    /// The `transition-*` longhands.
    pub transitions: Transitions,

    // Inherited.
    pub color: Color,
    /// The font properties: shared with the parent until one is set
    /// (Blink's `FontDescription`).
    pub font: Arc<FontDescription>,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub white_space_collapse: WhiteSpaceCollapse,
    pub text_wrap_mode: TextWrapMode,
    pub text_transform: TextTransform,
    pub list_style_position: ListStylePosition,
    /// Not inherited.
    pub text_overflow: TextOverflow,
    /// Not inherited.
    pub appearance: Appearance,
    /// What `appearance` is used as (style's adjustment sets it).
    pub effective_appearance: EffectiveAppearance,
    pub visibility: Visibility,
    pub pointer_events: PointerEvents,
    /// The element, or one around it in the flat tree, has a blocking
    /// `wheel` listener (one that isn't passive; the root element counts its
    /// document's and window's, an element at the top of a shadow tree its
    /// shadow root's): a wheel over its boxes waits for the main thread, as
    /// a listener there may cancel its scrolling. Not a CSS property:
    /// inherited as one, so a listener coming or going styles its element
    /// again and what is below it follows. Blink keeps it on the
    /// `LayoutObject` (`InsideBlockingWheelEventHandler`), which pre-paint
    /// updates down the layout tree; machopo's pre-paint walks fragments by
    /// their containing block.
    pub inside_blocking_wheel_event_handler: bool,
    pub border_collapse: BorderCollapse,
    pub caption_side: CaptionSide,
    pub empty_cells: EmptyCells,
    /// The inherited properties most elements leave as their parent's:
    /// shared with it until one is set (Blink's `StyleRareInheritedData`).
    pub rare_inherited: Arc<RareInherited>,
    pub svg: Arc<SvgStyle>,
    /// Custom properties, with `var()` substituted; they all inherit.
    pub variables: StyleVariables,
    /// A value of it came from a viewport unit (`vw`): it is styled again
    /// when the viewport changes (Blink's `HasViewportUnits`). Not
    /// inherited: a child takes the parent's computed px, and is styled
    /// again with it.
    pub has_viewport_units: bool,
    /// A property that doesn't inherit took its parent's value (`inherit`;
    /// Blink's `HasExplicitInheritance`): the element is styled again when
    /// its parent's non-inherited values change, not only its inherited
    /// ones. Not inherited.
    pub explicit_inheritance: bool,
    /// As written, whatever the style (css-ui 4: unlike a border's).
    pub outline_width: f32,
    pub outline_style: OutlineStyle,
    pub outline_color: StyleColor,
    pub outline_offset: f32,
    /// The non-inherited properties most elements leave initial (grid,
    /// animations, `border-radius`, counters, `content`): one
    /// shared copy until one is set (Blink's `StyleRareNonInheritedData`).
    pub rare: Arc<RareNonInherited>,
    /// The background layers' lists (`background-color` apart): shared
    /// until one is set (Blink's `StyleBackgroundData`).
    pub backgrounds: Arc<Backgrounds>,
}

thread_local! {
    /// Every property at its initial value, made once for each renderer
    /// thread: its lists are shared (Blink's `ComputedStyle::
    /// GetInitialStyleSingleton`, one in each renderer process).
    static INITIAL: ComputedStyle = ComputedStyle::make_initial();
}

impl ComputedStyle {
    /// Every property at its initial value.
    pub fn initial() -> Self {
        INITIAL.with(Self::clone)
    }

    fn make_initial() -> Self {
        Self {
            display: Display::Inline,
            original_display: Display::Inline,
            box_sizing: BoxSizing::ContentBox,
            width: Size::Auto,
            height: Size::Auto,
            min_width: LengthPercentageOrAuto::Auto,
            min_height: LengthPercentageOrAuto::Auto,
            max_width: LengthPercentageOrNone::None,
            max_height: LengthPercentageOrNone::None,
            margin: Sides::all(LengthPercentageOrAuto::LengthPercentage(
                LengthPercentage::ZERO,
            )),
            padding: Sides::all(LengthPercentage::ZERO),
            // `medium`, computed to 0 because the style is `none`.
            border_width: Sides::all(0.0),
            border_style: Sides::all(BorderStyle::None),
            border_color: Sides::all(StyleColor::CurrentColor),
            box_shadow: Arc::new([]),
            background_color: StyleColor::Rgba(Color::TRANSPARENT),
            text_decoration_line: TextDecorationLine::NONE,
            opacity: 1.0,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            transform: Arc::new([]),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: FlexBasis::Size(Size::Auto),
            order: 0,
            justify_content: ContentAlignment::Normal,
            align_content: ContentAlignment::Normal,
            align_items: ItemAlignment::Normal,
            align_self: None,
            row_gap: LengthPercentage::ZERO,
            column_gap: LengthPercentage::ZERO,
            justify_items: ItemAlignment::Normal,
            justify_self: None,
            z_index: None,
            effective_z_index: None,
            vertical_align: VerticalAlign::Baseline,
            table_layout: TableLayout::Auto,
            transitions: Transitions::initial(),
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            inset: Sides {
                top: LengthPercentageOrAuto::Auto,
                right: LengthPercentageOrAuto::Auto,
                bottom: LengthPercentageOrAuto::Auto,
                left: LengthPercentageOrAuto::Auto,
            },
            // `CanvasText`.
            color: Color::BLACK,
            font: Arc::new(FontDescription {
                // The standard font, as in Chromium.
                family: Arc::new([FontFamily::Generic(GenericFamily::Serif)]),
                size: 16.0,
                weight: 400.0,
                style: FontStyle::Normal,
                stretch: 100.0,
                variant_caps: FontVariantCaps::Normal,
                variant_ligatures: FontVariantLigatures::default(),
                variant_numeric: FontVariantNumeric::default(),
                variant_east_asian: FontVariantEastAsian::default(),
                variant_alternates: FontVariantAlternates::default(),
                variant_position: FontVariantPosition::Normal,
                variant_emoji: FontVariantEmoji::Normal,
                feature_settings: Arc::new([]),
                variation_settings: Arc::new([]),
                kerning: FontKerning::Auto,
                optical_sizing: true,
                size_adjust: FontSizeAdjust::None,
                language_override: None,
                synthesis_weight: true,
                synthesis_style: true,
                synthesis_small_caps: true,
                text_rendering: TextRendering::Auto,
                smoothing: FontSmoothing::Auto,
            }),
            line_height: LineHeight::Normal,
            text_align: TextAlign::Start,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            text_wrap_mode: TextWrapMode::Wrap,
            text_transform: TextTransform::None,
            list_style_position: ListStylePosition::Outside,
            text_overflow: TextOverflow::Clip,
            appearance: Appearance::None,
            effective_appearance: EffectiveAppearance::None,
            visibility: Visibility::Visible,
            pointer_events: PointerEvents::Auto,
            inside_blocking_wheel_event_handler: false,
            border_collapse: BorderCollapse::Separate,
            caption_side: CaptionSide::Top,
            empty_cells: EmptyCells::Show,
            rare_inherited: Arc::new(RareInherited {
                list_style_type: ListStyleType::Disc,
                letter_spacing: None,
                word_spacing: 0.0,
                tab_size: TabSize::Spaces(8.0),
                text_indent: LengthPercentage::Px(0.0),
                word_break: WordBreak::Normal,
                overflow_wrap: OverflowWrap::Normal,
                applied_text_decorations: Arc::new([]),
                text_shadow: Arc::new([]),
                text_fill_color: StyleColor::CurrentColor,
                text_underline_offset: None,
                text_underline_position: TextUnderlinePosition {
                    from_font: false,
                    under: false,
                    left: false,
                    right: false,
                },
                text_security: TextSecurity::None,
                border_spacing: BorderSpacing::ZERO,
                user_select: UserSelect::Auto,
                user_modify: UserModify::ReadOnly,
                hyphens: Hyphens::Manual,
                forced_color_adjust: ForcedColorAdjust::Auto,
                // Chrome's `LayoutTheme::TapHighlightColor`.
                tap_highlight_color: StyleColor::Rgba(Color::rgba(0, 0, 0, 46)),
                text_size_adjust: TextSizeAdjust::Auto,
                cursor: Cursor::initial(),
                caret_color: CaretColor::Auto,
                highlight: None,
            }),
            svg: Arc::new(SvgStyle::initial()),
            variables: StyleVariables::new(),
            has_viewport_units: false,
            explicit_inheritance: false,

            outline_width: 3.0,
            outline_style: OutlineStyle::Line(BorderStyle::None),
            outline_color: StyleColor::CurrentColor,
            outline_offset: 0.0,
            rare: Arc::new(RareNonInherited {
                animations: Animations::initial(),
                grid_template_columns: TrackList::none(),
                grid_template_rows: TrackList::none(),
                grid_template_areas: None,
                grid_auto_columns: Arc::new([TrackSize::Breadth(TrackBreadth::Auto)]),
                grid_auto_rows: Arc::new([TrackSize::Breadth(TrackBreadth::Auto)]),
                grid_auto_flow: GridAutoFlow::Row,
                grid_row_start: GridLine::Auto,
                grid_row_end: GridLine::Auto,
                grid_column_start: GridLine::Auto,
                grid_column_end: GridLine::Auto,
                border_radius: Corners::all(CornerRadius::ZERO),
                counter_reset: Arc::new([]),
                counter_increment: Arc::new([]),
                counter_set: Arc::new([]),
                content: Content::Normal,
                transform_origin: TransformOrigin::CENTER,
                transform_box: TransformBox::ViewBox,
                object_fit: ObjectFit::Fill,
                object_position: Position2D::CENTER,
                box_orient: BoxOrient::Horizontal,
                box_direction: BoxDirection::Normal,
                box_pack: BoxPack::Start,
                box_align: BoxAlign::Stretch,
                box_flex: 0.0,
                box_ordinal_group: 1,
                line_clamp: None,
                contain: Contain::NONE,
                clip: None,
                clip_path: ClipPath::None,
                text_decoration_style: TextDecorationStyle::Solid,
                text_decoration_color: StyleColor::CurrentColor,
                text_decoration_thickness: TextDecorationThickness::Auto,
                highlight_variables: None,
                filter: Arc::new([]),
                isolation: Isolation::Auto,
                specified_display_webkit_box: false,
                resize: Resize::None,
                scroll_behavior: ScrollBehavior::Auto,
                overflow_anchor: OverflowAnchor::Auto,
                border_image: BorderImage::initial(),
            }),
            backgrounds: Arc::new(Backgrounds {
                background_image: Arc::new([BackgroundImage::None]),
                background_position_x: Arc::new([PositionComponent::START]),
                background_position_y: Arc::new([PositionComponent::START]),
                background_size: Arc::new([BackgroundSize::AUTO]),
                background_repeat: Arc::new([(BackgroundRepeat::Repeat, BackgroundRepeat::Repeat)]),
                background_attachment: Arc::new([BackgroundAttachment::Scroll]),
                background_clip: Arc::new([BackgroundBox::BorderBox]),
                background_origin: Arc::new([BackgroundBox::PaddingBox]),
            }),
        }
    }

    /// The inherited properties of `parent`, everything else initial:
    /// where cascading an element starts.
    #[allow(clippy::clone_on_copy)]
    pub fn inherit(parent: &Self) -> Self {
        // The parent's SVG properties, if its non-inherited ones are
        // initial too: most elements' (and the root's parent's).
        let svg = SvgStyle::inherit(&parent.svg);
        let svg = if svg == *parent.svg {
            parent.svg.clone()
        } else {
            Arc::new(svg)
        };
        macro_rules! from_parent {
            ($($field:ident),*) => {
                Self {
                    $($field: parent.$field.clone(),)*
                    font: parent.font.clone(),
                    rare_inherited: parent.rare_inherited.clone(),
                    svg,
                    ..Self::initial()
                }
            };
        }
        inherited!(from_parent)
    }

    /// The rare non-inherited properties, to set one: this style's own
    /// copy of them (copied if shared).
    pub fn rare_mut(&mut self) -> &mut RareNonInherited {
        Arc::make_mut(&mut self.rare)
    }

    /// The font properties, to set one: this style's own copy of them
    /// (copied if shared).
    pub fn font_mut(&mut self) -> &mut FontDescription {
        Arc::make_mut(&mut self.font)
    }

    /// The rare inherited properties, to set one: this style's own copy of
    /// them (copied if shared).
    pub fn rare_inherited_mut(&mut self) -> &mut RareInherited {
        Arc::make_mut(&mut self.rare_inherited)
    }

    /// The background layers' lists, to set one: this style's own copy.
    pub fn backgrounds_mut(&mut self) -> &mut Backgrounds {
        Arc::make_mut(&mut self.backgrounds)
    }

    /// The SVG properties, to set one: this style's own copy.
    pub fn svg_mut(&mut self) -> &mut SvgStyle {
        Arc::make_mut(&mut self.svg)
    }

    /// Whether the inherited properties are the same in both: if they are,
    /// a child styled with either as its parent comes out the same, but for
    /// what it takes of the rest (`display`, for blockification, and what
    /// an explicit `inherit` takes: `explicit_inheritance`). Blink's
    /// `ComputedStyle::ComputeDifference` short of `kInherited`.
    pub fn same_inherited(&self, other: &Self) -> bool {
        macro_rules! same {
            ($($field:ident),*) => { true $(&& self.$field == other.$field)* };
        }
        inherited!(same)
            && (Arc::ptr_eq(&self.font, &other.font) || self.font == other.font)
            && (Arc::ptr_eq(&self.rare_inherited, &other.rare_inherited)
                || self.rare_inherited == other.rare_inherited)
            && self.svg.same_inherited(&other.svg)
    }

    /// The style of an anonymous block box inside a box with style
    /// `parent`. It keeps `text-indent`: whether its first line is its
    /// element's first formatted line, which is indented, is layout's to
    /// say (Blink's `LayoutBlockFlow::CanContainFirstFormattedLine`).
    pub fn anonymous_block(parent: &Self) -> Self {
        Self {
            display: Display::Block,
            ..Self::inherit(parent)
        }
    }

    /// Whether this element's box is a stacking context (no positioning
    /// yet): `opacity < 1`, a transform, a `clip-path` (css-masking 1), a
    /// `filter` or `isolation: isolate` (filter-effects 1, compositing 1),
    /// a `z-index` that applies, or layout or paint containment (css-contain
    /// 2; on a box it applies to: not a non-atomic inline one, nor a table
    /// row, row group or column). The root element's is the root stacking
    /// context.
    pub fn is_stacking_context(&self) -> bool {
        self.opacity < 1.0
            || !self.transform.is_empty()
            || self.rare.clip_path != ClipPath::None
            || !self.rare.filter.is_empty()
            || self.rare.isolation == Isolation::Isolate
            // A sticky box moves as a whole as its scrollport scrolls
            // (Blink's `StyleAdjuster` makes it one with `z-index: auto`).
            || self.position == Position::Sticky
            || self.effective_z_index.is_some()
            || self.contains_layout_or_paint()
    }

    /// Whether layout or paint containment applies to its box, if it has
    /// one of those it applies to.
    fn contains_layout_or_paint(&self) -> bool {
        let contain = self.rare.contain;
        (contain.contains(Contain::LAYOUT) || contain.contains(Contain::PAINT))
            && !matches!(
                self.display,
                Display::Inline
                    | Display::Table(
                        TableDisplay::Row
                            | TableDisplay::RowGroup
                            | TableDisplay::HeaderGroup
                            | TableDisplay::FooterGroup
                            | TableDisplay::Column
                            | TableDisplay::ColumnGroup
                    )
            )
    }

    /// Whether `position` takes the box out of the normal paint order:
    /// anything but `static`.
    pub fn is_positioned(&self) -> bool {
        self.position != Position::Static
    }

    /// Whether the box is out of flow, laid out by its containing block
    /// (Blink's `LayoutObject::IsOutOfFlowPositioned`).
    pub fn is_out_of_flow(&self) -> bool {
        matches!(self.position, Position::Absolute | Position::Fixed)
    }

    /// Whether the box stays where it is when its frame's viewport
    /// scrolls (`position: fixed`).
    pub fn is_fixed(&self) -> bool {
        self.position == Position::Fixed
    }

    /// Whether the box is sticky with an inset to keep (Blink's
    /// `HasStickyConstrainedPosition`): one with all four `auto` stays
    /// where the flow put it, as a relative one that isn't moved.
    pub fn is_sticky_constrained(&self) -> bool {
        let inset = &self.inset;
        self.position == Position::Sticky
            && [inset.top, inset.right, inset.bottom, inset.left]
                .iter()
                .any(|v| *v != LengthPercentageOrAuto::Auto)
    }

    /// Whether the box is the containing block of the absolutely
    /// positioned boxes below it (CSS 2 § 10.1: a positioned ancestor).
    /// The root element's box is one too, as the initial containing block.
    pub fn is_containing_block(&self) -> bool {
        self.is_positioned()
    }

    /// Whether the box is painted apart from the normal flow, in its
    /// stacking context's lists (Blink's `PaintLayer::IsStacked`): a
    /// stacking context, or a positioned box, which with `z-index: auto`
    /// is painted as if it were one (CSS 2 Appendix E step 8).
    pub fn is_stacked(&self) -> bool {
        self.is_stacking_context() || self.is_positioned()
    }

    /// Where a stacking context goes among its siblings (CSS 2 Appendix E):
    /// its `z-index`, 0 for `auto`.
    pub fn stack_level(&self) -> i32 {
        self.effective_z_index.unwrap_or(0)
    }

    /// Whether the box clips its content to its padding box. Such a box
    /// is a scroll container: its content can be scrolled, by the user
    /// only on an axis that is `auto` or `scroll`.
    pub fn clips_overflow(&self) -> bool {
        self.overflow_x != Overflow::Visible || self.overflow_y != Overflow::Visible
    }

    /// Whether it is a scroll container (css-overflow 3): it clips, and
    /// neither axis is `clip` (which computes to `hidden` next to one that
    /// scrolls).
    pub fn is_scroll_container(&self) -> bool {
        self.clips_overflow()
            && self.overflow_x != Overflow::Clip
            && self.overflow_y != Overflow::Clip
    }

    /// The number of lines its block formatting context is clamped to, if
    /// it is a line clamp container: a `-webkit-box` with a vertical
    /// `-webkit-line-clamp` (Blink's `ComputedStyle::HasLineClamp`).
    pub fn line_clamp(&self) -> Option<u32> {
        self.rare
            .line_clamp
            .filter(|_| self.rare.specified_display_webkit_box)
    }

    /// The background layers, top first: one per `background-image`, each
    /// other longhand's list repeated as needed (css-backgrounds 3 § 2.1;
    /// Blink's `FillLayer::FillUnsetProperties`).
    pub fn background_layers(&self) -> Vec<BackgroundLayer> {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "each background list has an item at least"
        )]
        fn nth<T: Clone>(list: &[T], i: usize) -> T {
            list[i % list.len()].clone()
        }
        (0..self.backgrounds.background_image.len())
            .map(|i| BackgroundLayer {
                image: self.backgrounds.background_image[i].clone(),
                position_x: nth(&self.backgrounds.background_position_x, i),
                position_y: nth(&self.backgrounds.background_position_y, i),
                size: nth(&self.backgrounds.background_size, i),
                repeat: nth(&self.backgrounds.background_repeat, i),
                attachment: nth(&self.backgrounds.background_attachment, i),
                clip: nth(&self.backgrounds.background_clip, i),
                origin: nth(&self.backgrounds.background_origin, i),
            })
            .collect()
    }

    /// Whether any layer has an image.
    pub fn has_background_image(&self) -> bool {
        self.backgrounds
            .background_image
            .iter()
            .any(|image| *image != BackgroundImage::None)
    }

    /// The URLs of the background's images shown at
    /// `device_scale_factor`, to load.
    pub fn background_image_urls(
        &self,
        device_scale_factor: f32,
    ) -> impl Iterator<Item = &ImageUrl> {
        self.backgrounds
            .background_image
            .iter()
            .filter_map(move |image| image.shown_url(device_scale_factor))
    }

    /// Whether a layer's image is an `image-set()`: which of its images
    /// shows depends on the device scale factor.
    pub fn has_background_image_set(&self) -> bool {
        self.backgrounds
            .background_image
            .iter()
            .any(|image| matches!(image, BackgroundImage::Set(_)))
    }

    /// Whether any corner is rounded.
    pub fn has_border_radius(&self) -> bool {
        self.rare.border_radius != Corners::all(CornerRadius::ZERO)
    }

    /// `color` with `currentcolor` resolved against this style.
    /// Whether the box paints and is hit (`visibility: visible`); its
    /// descendants decide for themselves.
    pub fn is_visible(&self) -> bool {
        self.visibility == Visibility::Visible
    }

    pub fn resolve(&self, color: StyleColor) -> Color {
        match color {
            StyleColor::CurrentColor => self.color,
            StyleColor::Rgba(color) => color,
        }
    }

    /// The decorations drawn on its text with `parent`'s drawn on the
    /// parent's: those, then its own (Blink's `ComputedStyle::
    /// ApplyTextDecorations`); its own alone for a box they don't go
    /// into (`parent` empty).
    pub fn applied_text_decorations_from(
        &self,
        parent: &Arc<[AppliedTextDecoration]>,
    ) -> Arc<[AppliedTextDecoration]> {
        if self.text_decoration_line.is_none() {
            return parent.clone();
        }
        let own = AppliedTextDecoration {
            lines: self.text_decoration_line,
            style: self.rare.text_decoration_style,
            color: self.resolve(self.rare.text_decoration_color),
            thickness: self.rare.text_decoration_thickness,
            underline_offset: self.rare_inherited.text_underline_offset,
        };
        parent.iter().copied().chain([own]).collect()
    }

    /// The `transform` functions as one matrix, the first outermost, the
    /// percentages against `size`, the reference box's (Blink's
    /// `ComputedStyle::ApplyTransform` without the transform origin): what
    /// `getComputedStyle` resolves `transform` to. Identity with no
    /// transform.
    pub fn transform_matrix(&self, size: SizeF) -> Transform {
        transform_list_matrix(&self.transform, size)
    }

    /// The transform about `transform-origin`, `reference` the reference
    /// box (`transform-box`) in the coordinates the transform applies in
    /// (Blink's `ComputedStyle::ApplyTransform` with the origin): what
    /// pre-paint puts on the element's box. `None` with no transform.
    pub fn transform_about_origin(&self, reference: RectF) -> Option<Transform> {
        if self.transform.is_empty() {
            return None;
        }
        let origin = self.rare.transform_origin;
        let x = reference.x + origin.x.resolve(reference.width);
        let y = reference.y + origin.y.resolve(reference.height);
        Some(
            Transform::translate(x, y)
                .then(&self.transform_matrix(reference.size()))
                .then(&Transform::translate(-x, -y)),
        )
    }
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self::initial()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A length-percentage resolved is in the range a computed length is:
    /// a percentage of a big basis, or a big sum, saturates.
    #[test]
    fn resolved_lengths_are_in_range() {
        let max = MAX_CSS_LENGTH;
        assert_eq!(LengthPercentage::Percent(max).resolve(1e30), max);
        assert_eq!(LengthPercentage::Percent(-max).resolve(1e30), -max);
        assert_eq!(LengthPercentage::Percent(50.0).resolve(f32::NAN), 0.0);
        let calc = |px, percent| LengthPercentage::Calc {
            px,
            percent,
            non_negative: false,
        };
        assert_eq!(calc(max, max).resolve(1e6), max);
        assert_eq!(calc(10.0, 50.0).resolve(100.0), 60.0);
        assert_eq!(LineHeight::number_px(f32::MAX, MAX_FONT_SIZE), max);
        assert_eq!(LineHeight::number_px(1.5, 20.0), 30.0);
    }

    /// The image an `image-set()` shows at 1x, 2x and 3x, as Chrome 152
    /// picks it: the lowest resolution at least the scale, else the
    /// highest, the first written of equals; an unsupported `type()`
    /// skipped (its MIME type ASCII case-insensitive); none at 0dppx.
    #[test]
    fn an_image_set_shows_the_image_chrome_picks() {
        let set = |options: &[(&str, f32, Option<&str>)]| {
            BackgroundImage::Set(
                options
                    .iter()
                    .map(|&(url, resolution, mime_type)| ImageSetOption {
                        image: BackgroundImage::Url(url.into()),
                        resolution,
                        mime_type: mime_type.map(Into::into),
                    })
                    .collect(),
            )
        };
        let shown = |image: &BackgroundImage, scale| -> Option<(String, f32)> {
            image.shown(scale).map(|(image, resolution)| match image {
                BackgroundImage::Url(url) => (url.url.to_string(), resolution),
                image => panic!("{image:?}"),
            })
        };
        // What shows at 1x, 2x and 3x: the URL and its resolution.
        type Shown = [Option<(&'static str, f32)>; 3];
        let cases: [(BackgroundImage, Shown); 11] = [
            (
                set(&[("a", 1.0, None), ("b", 2.0, None)]),
                [Some(("a", 1.0)), Some(("b", 2.0)), Some(("b", 2.0))],
            ),
            (set(&[("b", 2.0, None)]), [Some(("b", 2.0)); 3]),
            (
                set(&[("a", 1.5, None), ("c", 3.0, None)]),
                [Some(("a", 1.5)), Some(("c", 3.0)), Some(("c", 3.0))],
            ),
            (set(&[("b", 0.5, None)]), [Some(("b", 0.5)); 3]),
            (
                set(&[("a", 1.0, Some("image/x-nope")), ("b", 2.0, None)]),
                [Some(("b", 2.0)); 3],
            ),
            (set(&[("a", 1.0, Some("image/x-nope"))]), [None; 3]),
            (set(&[("a", 1.0, Some("IMAGE/PNG"))]), [Some(("a", 1.0)); 3]),
            // A type machopo doesn't decode, though Chrome does.
            (
                set(&[("a", 1.0, Some("image/webp")), ("b", 2.0, None)]),
                [Some(("b", 2.0)); 3],
            ),
            (set(&[("a", 0.0, None)]), [None; 3]),
            (
                set(&[("a", 1.0, None), ("b", 1.0, None)]),
                [Some(("a", 1.0)); 3],
            ),
            (
                set(&[("c", 3.0, None), ("b", 2.0, None)]),
                [Some(("b", 2.0)), Some(("b", 2.0)), Some(("c", 3.0))],
            ),
        ];
        for (image, want) in cases {
            for (scale, want) in [1.0, 2.0, 3.0].into_iter().zip(want) {
                let want = want.map(|(url, resolution)| (url.to_owned(), resolution));
                assert_eq!(shown(&image, scale), want, "{image:?} at {scale}x");
            }
        }
        // Not a set: itself, at 1dppx.
        let url = BackgroundImage::Url("u".into());
        assert_eq!(shown(&url, 2.0), Some(("u".to_owned(), 1.0)));
        assert_eq!(BackgroundImage::None.shown(2.0), None);
        let options = set(&[("a", 1.0, None), ("b", 2.0, None)]);
        assert_eq!(options.shown_url(2.0).map(|url| &*url.url), Some("b"));
    }
}
