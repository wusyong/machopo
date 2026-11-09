//! The cascade of an element's declaration blocks into its computed
//! style (Blink's `StyleCascade` and `StyleAdjuster`), and the matched
//! properties cache: what doesn't read the DOM. The blocks come from the
//! rules matched, the presentation attributes and the `style` attribute,
//! which the style engine (`style`) collects from the DOM.

use std::cell::RefCell;
use std::sync::Arc;

use computed_style::{
    BoxOrient, ComputedStyle, Content, Display, Float, HighlightStyle, LengthPercentage,
    ListStylePosition, PseudoId, StyleColor,
};
use css::properties::{self, CssWideKeyword, Declaration, LonghandId, Value};
use css::stylesheet::Origin;
use css::values::{ComputeContext, SpecifiedColor};
use css::variables::{CustomDeclared, UnparsedValue, compute_custom_properties};
use css::{CssPropertyValueSet, StyleRule};
use gfx::SizeF;

use crate::theme;

/// What every element's cascade in a recalc shares: the frame's viewport
/// (viewport units), the platform's system colors and the document's link
/// colors.
#[derive(Clone, Copy, PartialEq)]
pub struct Environment {
    pub viewport: SizeF,
    pub system_colors: gfx::SystemColors,
    pub link_colors: css::values::TextLinkColors,
}

/// Where a block of declarations sits in the cascade: sorted by these
/// fields in order, later wins (css-cascade's cascade sorting order, without
/// layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CascadePriority {
    /// UA normal < author normal < animations < author `!important` < UA
    /// `!important` (css-cascade 4's origins; transitions, the highest,
    /// are applied after the cascade).
    pub level: u8,
    /// Which tree's: for normal declarations the outer tree's win, for
    /// `!important` ones the inner tree's (css-scoping 3; Blink's
    /// `EncodeTreeOrder`). See [`CascadePriority::tree`].
    pub tree: u32,
    /// A `style` attribute beats any rule of the same level.
    pub style_attribute: bool,
    pub specificity: u32,
    /// Index of the rule in cascade order.
    pub order: usize,
}

impl CascadePriority {
    /// The `tree` of a declaration of the element's `tree_order`th tree
    /// (see `RuleSource`).
    pub fn tree(tree_order: u32, important: bool) -> u32 {
        // `u32::MAX - tree_order`, which is its bitwise not.
        if important { tree_order } else { !tree_order }
    }

    pub fn level(origin: Origin, important: bool) -> u8 {
        match (origin, important) {
            (Origin::UserAgent, false) => 0,
            (Origin::Author, false) => 1,
            (Origin::Author, true) => 3,
            (Origin::UserAgent, true) => 4,
        }
    }

    /// The animation origin's: a CSS animation's keyframe's declarations.
    pub const ANIMATION: Self = Self {
        level: 2,
        tree: 0,
        style_attribute: false,
        specificity: 0,
        order: 0,
    };
}

/// The `::selection` style of an element whose style is `element`, from
/// its own `::selection` rules' `blocks` and the parent's highlight style
/// `parent` (the root's initial one if `None`): a property they declare
/// as they declare it (`inherit` the parent's), and the others the
/// parent's, as all of a highlight's inherit (css-pseudo 4's highlight
/// inheritance; Blink's `StyleResolver::ResolveHighlightStyles`). Lengths
/// are of the element's font. `color: currentcolor` stays the keyword
/// (the originating element's `color`, each element's own as it
/// inherits), which a computed `color` can't be. With its custom
/// properties if the blocks declare some (`RareNonInherited::
/// highlight_variables`).
pub fn cascade_highlight(
    mut blocks: Blocks<'_>,
    element: &ComputedStyle,
    parent: Option<&HighlightStyle>,
    root_font_size: f32,
    env: Environment,
) -> (HighlightStyle, Option<computed_style::StyleVariables>) {
    blocks.sort_by_key(|(priority, ..)| *priority);
    let inherited = parent.cloned().unwrap_or_else(HighlightStyle::initial);
    // What `inherit` takes: the element's style with the parent's
    // highlight's values.
    let mut from = element.clone();
    inherited.apply_to(&mut from);
    let cascaded = cascade_declarations(
        &blocks,
        &from,
        root_font_size,
        env,
        LonghandId::applies_to_highlight,
        None,
    )
    .style;
    // Each property's winning declaration, as `cascade_declarations`
    // finds it: from the highest priority down, each block from its end.
    let winner = |id: LonghandId| {
        blocks.iter().rev().find_map(|(_, block, important, _)| {
            block
                .declarations(*important)
                .rev()
                .find(|declaration| declaration.id.physical() == id)
        })
    };
    let own = HighlightStyle::of(&cascaded);
    let declared = |id: LonghandId| winner(id).is_some();
    let color = match winner(LonghandId::Color).map(|declaration| &declaration.value) {
        None | Some(Value::CssWide(CssWideKeyword::Inherit | CssWideKeyword::Unset)) => {
            inherited.color
        }
        Some(Value::Color(SpecifiedColor::CurrentColor)) => StyleColor::CurrentColor,
        Some(_) => own.color,
    };
    let mut highlight = inherited;
    highlight.color = color;
    highlight.author_colors |= declared(LonghandId::Color) || declared(LonghandId::BackgroundColor);
    if declared(LonghandId::BackgroundColor) {
        highlight.background_color = own.background_color;
    }
    if declared(LonghandId::TextDecorationLine) {
        highlight.text_decoration_line = own.text_decoration_line;
    }
    if declared(LonghandId::TextDecorationStyle) {
        highlight.text_decoration_style = own.text_decoration_style;
    }
    if declared(LonghandId::TextDecorationColor) {
        highlight.text_decoration_color = own.text_decoration_color;
    }
    if declared(LonghandId::TextDecorationThickness) {
        highlight.text_decoration_thickness = own.text_decoration_thickness;
    }
    if declared(LonghandId::TextUnderlineOffset) {
        highlight.text_underline_offset = own.text_underline_offset;
    }
    if declared(LonghandId::TextShadow) {
        highlight.text_shadow = own.text_shadow;
    }
    if declared(LonghandId::Fill) {
        highlight.fill = own.fill;
    }
    if declared(LonghandId::Stroke) {
        highlight.stroke = own.stroke;
    }
    if declared(LonghandId::StrokeWidth) {
        highlight.stroke_width = own.stroke_width;
    }
    let custom = blocks
        .iter()
        .any(|(_, block, important, _)| block.custom_declarations(*important).next().is_some());
    (highlight, custom.then(|| cascaded.variables.clone()))
}

/// The style of a pseudo-element no rule matches, of an element whose
/// style is `element`: what cascading no declarations gives (Blink's
/// `EnsureComputedStyle` computes it when asked), with no `content` (a
/// marker's `normal`).
pub fn pseudo_style_without_rules(element: &ComputedStyle, pseudo: PseudoId) -> ComputedStyle {
    // No declaration uses a font size, the viewport or a system color.
    let mut style = Arc::unwrap_or_clone(cascade_blocks(
        Blocks::new(),
        element,
        element,
        Position {
            is_root: false,
            in_flex_or_grid: element.display.blockifies_children(),
            in_webkit_box: element.display.is_webkit_box(),
            control: None,
            blocking_wheel: false,
            pseudo: None,
        },
        element.font.size,
        Environment {
            viewport: SizeF::default(),
            system_colors: gfx::SystemColors::DEFAULT,
            link_colors: css::values::TextLinkColors::DEFAULT,
        },
        |_| true,
        None,
        None,
        None,
    ));
    if pseudo != PseudoId::Marker {
        style.rare_mut().content = Content::None;
    }
    style
}

/// The computed style the declaration `blocks` give an element whose
/// parent's style is `parent` (a pseudo-element's is its element's), of
/// the properties that `applies` says apply to it. `layout_parent` is the
/// style of the box its box would go in: `parent`'s, or for a child of a
/// `display: contents` element, its nearest ancestor's that isn't one
/// (Blink's `layout_parent_style`). `control` is the form control the
/// element is, whose native look the declarations decide. `animated_font_size`
/// is an animation's `font-size`, which wins over the declarations. With
/// `cache`, what the declarations give is looked for there first, and kept
/// there (with `shared_parent`, `parent` as it is shared, if it is).
#[allow(clippy::too_many_arguments)]
pub fn cascade_blocks(
    mut blocks: Blocks<'_>,
    parent: &ComputedStyle,
    layout_parent: &ComputedStyle,
    position: Position,
    root_font_size: f32,
    env: Environment,
    applies: fn(LonghandId) -> bool,
    animated_font_size: Option<f32>,
    shared_parent: Option<&Arc<ComputedStyle>>,
    cache: Option<&RefCell<MatchedPropertiesCache>>,
) -> Arc<ComputedStyle> {
    blocks.sort_by_key(|(priority, ..)| *priority);
    let declarations = || {
        cascade_declarations(
            &blocks,
            parent,
            root_font_size,
            env,
            applies,
            animated_font_size,
        )
    };
    let styled = || Arc::new(adjust(declarations(), parent, layout_parent, position));
    let cache = cache.filter(|_| {
        animated_font_size.is_none() && MatchedPropertiesCache::takes(&blocks, position)
    });
    let Some(cache) = cache else {
        return styled();
    };
    let key = CacheKey {
        blocks: &blocks,
        parent,
        shared_parent,
        position,
        root_font_size,
        env,
        applies,
    };
    let found = cache.borrow_mut().find(&key);
    match found {
        Some(style) => {
            // What the cache gives is what cascading gives, adjusted.
            debug_assert_eq!(*style, *styled());
            style
        }
        None => {
            let style = styled();
            cache.borrow_mut().add(&key, &style);
            style
        }
    }
}

/// What the adjustments of a cascade (`adjust`) take of where its element
/// is and what it is: part of a matched properties cache key, so that its
/// entries are adjusted and shared as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub is_root: bool,
    /// Its layout parent is a flex or grid container: it is blockified
    /// and its `z-index` applies.
    pub in_flex_or_grid: bool,
    /// Its layout parent is a `-webkit-box`: its `z-index` applies, but it
    /// isn't blockified (Blink's `BlockifiesChildren` leaves `-webkit-box`
    /// out; its inline content goes into anonymous items).
    pub in_webkit_box: bool,
    /// The form control it is, if any (a pseudo-element is none).
    pub control: Option<theme::Control>,
    /// It has a blocking `wheel` listener of its own (a pseudo-element
    /// inherits its element's): what it adds to what it inherits of
    /// `inside_blocking_wheel_event_handler`.
    pub blocking_wheel: bool,
    /// The pseudo-element it is of its parent, if it is one.
    pub pseudo: Option<PseudoId>,
}

/// What cascading declaration blocks gives, before the adjustments for
/// where the element is (Blink's style before `StyleAdjuster`): what the
/// matched properties cache keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct Cascaded {
    pub style: ComputedStyle,
    /// What the author set of a form control's background and border.
    pub author: theme::AuthorStyled,
}

/// The cascade of `blocks` (sorted), as [`cascade_blocks`] says, short of
/// the adjustments.
pub fn cascade_declarations(
    blocks: &[Block<'_>],
    parent: &ComputedStyle,
    root_font_size: f32,
    env: Environment,
    applies: fn(LonghandId) -> bool,
    animated_font_size: Option<f32>,
) -> Cascaded {
    // Each property's winning declaration (Blink's `CascadeMap`): the
    // first met from the highest priority down, as a block has each
    // property at most once for each importance. A flow-relative property
    // and its physical one share a place (Blink resolves the surrogate to
    // the physical property first), so within a block the later of the
    // two wins: each block is read from its end.
    // With whether an author's (a level of theirs) declaration it is.
    // On the stack: no allocation for each element. Which have one is kept
    // apart, so that what follows goes through those alone, not through
    // every longhand.
    let mut winners: [Option<(&Declaration, bool)>; LonghandId::COUNT] = [None; LonghandId::COUNT];
    let mut declared = Declared::default();
    let author_levels = [
        CascadePriority::level(Origin::Author, false),
        CascadePriority::level(Origin::Author, true),
    ];
    let mut custom: Vec<(&Arc<str>, CustomDeclared<'_>)> = Vec::new();
    let mut custom_seen: rustc_hash::FxHashSet<&str> = Default::default();
    for (priority, block, important, _) in blocks.iter().rev() {
        let author = author_levels.contains(&priority.level);
        for declaration in block.declarations(*important).rev() {
            let id = declaration.id.physical();
            let winner = &mut winners[id.index()];
            if winner.is_none() && applies(id) {
                *winner = Some((declaration, author));
                declared.insert(id.index());
            }
        }
        for (name, value) in block.custom_declarations(*important) {
            if custom_seen.insert(name) {
                custom.push((name, CustomDeclared::new(value)));
            }
        }
    }

    // Custom properties first: the others' `var()`s read them. Then
    // `font-size`: every other `em` is relative to it (Blink's
    // high-priority properties).
    let mut style = properties::cascade_start(parent);
    style.variables = compute_custom_properties(&parent.variables, &custom);
    let variables = style.variables.clone();
    let mut substituted: Vec<(Arc<UnparsedValue>, Option<Vec<Declaration>>)> = Vec::new();
    let mut apply = |style: &mut ComputedStyle, declaration: &Declaration, cx: &ComputeContext| {
        let Value::Unparsed(unparsed) = &declaration.value else {
            properties::apply(style, declaration, parent, cx);
            return;
        };
        // A shorthand's longhands share one value: substitute and parse it
        // once for them all.
        let i = match substituted
            .iter()
            .position(|(u, _)| Arc::ptr_eq(u, unparsed))
        {
            Some(i) => i,
            None => {
                let resolved = unparsed.resolve(declaration.id, &variables);
                let index = substituted.len();
                substituted.push((unparsed.clone(), resolved));
                index
            }
        };
        let resolved = substituted[i]
            .1
            .as_ref()
            .and_then(|declarations| declarations.iter().find(|d| d.id == declaration.id));
        // Invalid at computed-value time: `unset`, which is where the
        // cascade started (inherited, or initial), as only the winning
        // declaration of each property is applied.
        if let Some(resolved) = resolved {
            properties::apply(style, resolved, parent, cx);
        }
    };
    let viewport_used = std::cell::Cell::new(false);
    let font_cx = ComputeContext {
        font_size: parent.font.size,
        parent_font_size: parent.font.size,
        root_font_size,
        viewport: Some((env.viewport, &viewport_used)),
        system_colors: &env.system_colors,
        link_colors: env.link_colors,
    };
    // An animation's `font-size` wins over every declaration (it is
    // applied over the cascade's, `!important` ones too, which a CSS
    // animation's keyframes already have), and the `em`s follow it.
    if let Some(font_size) = animated_font_size {
        if style.font.size != font_size {
            style.font_mut().size = font_size;
        }
    } else if let Some((declaration, ..)) = winners[LonghandId::FontSize.index()] {
        apply(&mut style, declaration, &font_cx);
    }
    for index in declared.iter() {
        let Some((declaration, _)) = winners[index] else {
            unreachable!("declared")
        };
        if declaration.id == LonghandId::FontSize {
            continue;
        }
        let cx = ComputeContext {
            font_size: style.font.size,
            ..font_cx
        };
        apply(&mut style, declaration, &cx);
    }
    properties::finish(&mut style, parent);
    // What the author set of a form control's background and border
    // (Blink's `HasAuthorBackground`, `HasAuthorBorder`,
    // `HasAuthorBorderRadius`): with them it is drawn as CSS does.
    let mut author = theme::AuthorStyled::default();
    for index in declared.iter() {
        if let Some((.., true)) = winners[index] {
            let id = LonghandId::ALL[index];
            author.background |= id.is_background();
            author.border |= id.is_border();
            author.border_radius |= id.is_border_radius();
        }
    }
    style.has_viewport_units = viewport_used.get();
    Cascaded { style, author }
}

/// The longhands a cascade has a winning declaration for (Blink's
/// `CascadeMap::native_property_bits_`), by their index: a few for most
/// elements, of some two hundred and fifty longhands.
#[derive(Default)]
pub struct Declared([u64; Declared::WORDS]);

impl Declared {
    pub const WORDS: usize = LonghandId::COUNT.div_ceil(64);

    pub fn insert(&mut self, index: usize) {
        self.0[index / 64] |= 1 << (index % 64);
    }

    /// The indexes, in order.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "`bits` isn't 0 there; `word * 64 + bit` is an index that was inserted"
    )]
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(word, &bits)| {
            let mut bits = bits;
            std::iter::from_fn(move || {
                (bits != 0).then(|| {
                    let bit = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    word * 64 + bit
                })
            })
        })
    }
}

/// `cascaded`'s adjustments for where the element is and what it is
/// (Blink's `StyleAdjuster::AdjustComputedStyle`), as [`cascade_blocks`]
/// says.
pub fn adjust(
    cascaded: Cascaded,
    parent: &ComputedStyle,
    layout_parent: &ComputedStyle,
    position: Position,
) -> ComputedStyle {
    let Position {
        is_root,
        in_flex_or_grid,
        in_webkit_box,
        control,
        blocking_wheel,
        pseudo,
    } = position;
    debug_assert_eq!(in_flex_or_grid, layout_parent.display.blockifies_children());
    debug_assert_eq!(in_webkit_box, layout_parent.display.is_webkit_box());
    let Cascaded { mut style, author } = cascaded;
    style.inside_blocking_wheel_event_handler |= blocking_wheel;
    style.effective_appearance = theme::effective_appearance(control, style.appearance, author);
    if control.is_some_and(theme::Control::is_menu_list_inner) {
        theme::adjust_menu_list_inner(&mut style, parent);
    }
    // The theme's `display` comes after the text decorations' check, as
    // Blink adjusts it after `StyleAdjuster::AdjustComputedStyle`.
    let mut display = style.display;
    theme::adjust_style(&mut style);
    style.original_display = style.display;
    // An out-of-flow box doesn't float (CSS 2 § 9.7; Blink's
    // `StyleAdjuster::AdjustComputedStyle`).
    if style.is_out_of_flow() {
        style.float = Float::None;
    }
    // The root element, flex and grid items, out-of-flow boxes and floats
    // are blockified (css-display, "automatic box type transformations";
    // Blink's `StyleAdjuster::AdjustStyleForDisplay`). A container's
    // children are styled again with it, so they follow when its `display`
    // changes.
    if is_root || in_flex_or_grid || style.is_out_of_flow() || style.float != Float::None {
        style.display = style.display.blockified();
        display = display.blockified();
    }
    // An ancestor's text decorations aren't drawn in an atomic inline, a
    // float or an out-of-flow box, which start with their own (css-text-
    // decor 3, "Line Decoration"; Blink's `StopPropagateTextDecorations`).
    if display.is_atomic_inline() || style.float != Float::None || style.is_out_of_flow() {
        let decorations = style.applied_text_decorations_from(&Default::default());
        if decorations != style.rare_inherited.applied_text_decorations {
            style.rare_inherited_mut().applied_text_decorations = decorations;
        }
    }
    // `contents` isn't blockified, but the root element has a box
    // (css-display 3).
    if is_root && style.display == Display::Contents {
        style.display = Display::Block;
    }
    // A `-webkit-box` with a vertical `-webkit-line-clamp` is a line clamp
    // container: a `flow-root` (an `inline-block`), which remembers it was
    // a `-webkit-box` (Blink's `StyleAdjuster::AdjustComputedStyle` and
    // `IsSpecifiedDisplayWebkitBox`). Only that box clamps: one specified
    // `flow-root` doesn't.
    if style.rare.box_orient == BoxOrient::Vertical
        && style.rare.line_clamp.is_some()
        && style.display.is_webkit_box()
    {
        let display = match style.display {
            Display::WebkitBox => Display::FlowRoot,
            _ => Display::InlineBlock,
        };
        style.display = display;
        style.rare_mut().specified_display_webkit_box = true;
    }
    // `z-index` applies to positioned boxes and to flex and grid items, a
    // `-webkit-box`'s items too: Blink's `StyleAdjuster` sets its
    // effective value.
    style.effective_z_index = if style.is_positioned() || in_flex_or_grid || in_webkit_box {
        style.z_index
    } else {
        None
    };
    match pseudo {
        // A marker is laid out as its position says, whatever its
        // `display` (which doesn't apply to it): an outside one as an
        // inline-block, an inside one as an inline box; and it doesn't
        // take its list item's indent (Blink's `StyleAdjuster::
        // AdjustStyleForMarker`). Its `content: normal` is its
        // `list-style-type`'s.
        Some(PseudoId::Marker) => {
            style.display = match parent.list_style_position {
                ListStylePosition::Outside => Display::InlineBlock,
                ListStylePosition::Inside => Display::Inline,
            };
            if style.rare_inherited.text_indent != LengthPercentage::Px(0.0) {
                style.rare_inherited_mut().text_indent = LengthPercentage::Px(0.0);
            }
        }
        // `normal` is `none` on `::before` and `::after` (css-content 3).
        Some(_) if style.rare.content == Content::Normal => {
            style.rare_mut().content = Content::None
        }
        _ => {}
    }
    style
}

/// A declaration block in the cascade: where it sorts, whether it adds its
/// `!important` declarations, what its `url()`s resolve against, and whose
/// it is.
pub type Block<'a> = (
    CascadePriority,
    &'a CssPropertyValueSet,
    bool,
    BlockSource<'a>,
);

/// An element's (or pseudo-element's) blocks, on the stack while they are
/// as few as most elements have.
pub type Blocks<'a> = smallvec::SmallVec<[Block<'a>; 16]>;

/// Whose a declaration block is, for the matched properties cache.
#[derive(Clone, Copy)]
pub enum BlockSource<'a> {
    /// A rule's: one rule's block is always the same.
    Rule(&'a Arc<StyleRule>),
    /// The presentational hints, made for the element, with a digest of
    /// the attributes they are of (`hints_digest`).
    Hints(u64),
    /// A `style` attribute's, each element's own, or an animation's
    /// keyframe (what an animation cascades again isn't another
    /// element's): never kept.
    Own,
}

#[allow(clippy::too_many_arguments)]
pub fn push_block<'a>(
    blocks: &mut Blocks<'a>,
    origin: Origin,
    tree_order: u32,
    style_attribute: bool,
    specificity: u32,
    order: usize,
    block: &'a CssPropertyValueSet,
    source: BlockSource<'a>,
) {
    for important in [false, true] {
        if important && !block.has_important() {
            continue;
        }
        let priority = CascadePriority {
            level: CascadePriority::level(origin, important),
            tree: CascadePriority::tree(tree_order, important),
            style_attribute,
            specificity,
            order,
        };
        blocks.push((priority, block, important, source));
    }
}

/// What cascading sets of declaration blocks gave (Blink's
/// `MatchedPropertiesCache`), for an element (or a pseudo-element) whose
/// declarations, parent's inherited values and environment are those of
/// one cascaded before: elements alike (a list's items, each a repeated
/// component) cascade once. As Blink's, one cascade for each hash of the
/// declarations, the last; kept across recalcs. An entry keeps the rules
/// it is of: no other block is at their address while it is here (their
/// `url()`s resolved as they were parsed). It is cleared when it has too
/// many, and when the document's sheets change (a shadow tree's sheets
/// aren't looked at: many come and go).
#[derive(Default)]
pub struct MatchedPropertiesCache {
    pub entries: rustc_hash::FxHashMap<u64, CacheEntry>,
    /// The hashes of cascades looked for once and not found: one is kept
    /// when it is looked for again (one that never is, of an element alike
    /// no other, isn't copied).
    pub missed: rustc_hash::FxHashSet<u64>,
    /// How many cascades it has given, so far (for tests).
    pub hits: usize,
}

/// How many entries the matched properties cache keeps before it starts
/// again.
pub const MATCHED_PROPERTIES_CACHE_SIZE: usize = 4096;

/// What a cascade is looked for in the matched properties cache with.
pub struct CacheKey<'k, 'a> {
    pub blocks: &'k [Block<'a>],
    pub parent: &'k ComputedStyle,
    pub shared_parent: Option<&'k Arc<ComputedStyle>>,
    pub position: Position,
    pub root_font_size: f32,
    pub env: Environment,
    pub applies: fn(LonghandId) -> bool,
}

/// A cascade the matched properties cache has: its key's, owned.
pub struct CacheEntry {
    pub blocks: Vec<CachedBlock>,
    /// Of which only the inherited values count (all the cascade reads of
    /// it): the same one has them.
    pub parent: Arc<ComputedStyle>,
    pub position: Position,
    pub root_font_size: f32,
    pub env: Environment,
    pub applies: fn(LonghandId) -> bool,
    /// Adjusted for its position: what each element with the key gets,
    /// shared.
    pub style: Arc<ComputedStyle>,
}

/// A block of a cached cascade.
pub struct CachedBlock {
    pub priority: CascadePriority,
    pub important: bool,
    pub source: CachedSource,
}

/// Whose a cached block is: a rule's (the same rule), or the
/// presentational hints (the same declarations).
pub enum CachedSource {
    Rule(Arc<StyleRule>),
    Hints(CssPropertyValueSet),
}

impl MatchedPropertiesCache {
    /// Whether what `blocks` give may be kept: not with a block of the
    /// element's own, nor for a menu list's inner element, whose
    /// adjustments take its select's values that aren't inherited.
    pub fn takes(blocks: &[Block<'_>], position: Position) -> bool {
        !position
            .control
            .is_some_and(theme::Control::is_menu_list_inner)
            && blocks
                .iter()
                .all(|(.., source)| !matches!(source, BlockSource::Own))
    }

    pub fn hash(key: &CacheKey<'_, '_>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        for (priority, _, important, source) in key.blocks {
            priority.hash(&mut hasher);
            important.hash(&mut hasher);
            match source {
                BlockSource::Rule(rule) => Arc::as_ptr(rule).hash(&mut hasher),
                BlockSource::Hints(digest) => digest.hash(&mut hasher),
                BlockSource::Own => {}
            }
        }
        // Each position its own entry: a list's items in and out of a flex
        // container don't take turns.
        let Position {
            is_root,
            in_flex_or_grid,
            in_webkit_box,
            control,
            blocking_wheel,
            pseudo,
        } = key.position;
        (
            is_root,
            in_flex_or_grid,
            in_webkit_box,
            control,
            blocking_wheel,
            pseudo,
        )
            .hash(&mut hasher);
        hasher.finish()
    }

    pub fn find(&mut self, key: &CacheKey<'_, '_>) -> Option<Arc<ComputedStyle>> {
        let found = self
            .entries
            .get(&Self::hash(key))
            .filter(|entry| entry.matches(key))
            .map(|entry| entry.style.clone());
        // A statistic: wrapping only miscounts.
        self.hits = self.hits.wrapping_add(usize::from(found.is_some()));
        found
    }

    /// Keeps what `key`'s blocks gave, in place of what another cascade of
    /// the same hash gave, but for a value taken with `inherit` of a
    /// property that isn't inherited: then the parent's other values count
    /// too.
    pub fn add(&mut self, key: &CacheKey<'_, '_>, style: &Arc<ComputedStyle>) {
        if style.explicit_inheritance {
            return;
        }
        let hash = Self::hash(key);
        if !self.entries.contains_key(&hash) && self.missed.insert(hash) {
            return;
        }
        if self.entries.len() >= MATCHED_PROPERTIES_CACHE_SIZE
            || self.missed.len() >= MATCHED_PROPERTIES_CACHE_SIZE
        {
            self.clear();
        }
        let blocks = key
            .blocks
            .iter()
            .map(|(priority, block, important, source)| CachedBlock {
                priority: *priority,
                important: *important,
                source: match source {
                    BlockSource::Rule(rule) => CachedSource::Rule((*rule).clone()),
                    BlockSource::Hints(_) | BlockSource::Own => {
                        CachedSource::Hints((*block).clone())
                    }
                },
            })
            .collect();
        self.entries.insert(
            hash,
            CacheEntry {
                blocks,
                parent: key
                    .shared_parent
                    .cloned()
                    .unwrap_or_else(|| Arc::new(key.parent.clone())),
                position: key.position,
                root_font_size: key.root_font_size,
                env: key.env,
                applies: key.applies,
                style: style.clone(),
            },
        );
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.missed.clear();
    }
}

impl CacheEntry {
    pub fn matches(&self, key: &CacheKey<'_, '_>) -> bool {
        self.blocks.len() == key.blocks.len()
            && self.position == key.position
            && self.root_font_size.to_bits() == key.root_font_size.to_bits()
            && self.env == key.env
            && std::ptr::fn_addr_eq(self.applies, key.applies)
            && self.blocks.iter().zip(key.blocks).all(
                |(cached, (priority, block, important, source))| {
                    cached.priority == *priority
                        && cached.important == *important
                        && match (&cached.source, source) {
                            (CachedSource::Rule(cached), BlockSource::Rule(rule)) => {
                                Arc::ptr_eq(cached, rule)
                            }
                            (CachedSource::Hints(cached), BlockSource::Hints(_)) => {
                                cached == *block
                            }
                            _ => false,
                        }
                },
            )
            && (key
                .shared_parent
                .is_some_and(|parent| Arc::ptr_eq(parent, &self.parent))
                || self.parent.same_inherited(key.parent))
    }
}
