//! The supported properties: parsing declarations and applying them to a
//! `ComputedStyle`.
//!
//! The box, border, background, effect (`opacity`, `overflow`,
//! `transform`, `visibility`), flex, grid, text, table and SVG properties.
//! The text ones, `color`, `visibility`, the table ones but
//! `table-layout`, and SVG's painting ones are inherited.

// `Display`, `BoxSizing`, `Color`, the table properties and others also
// name `LonghandId` variants, so the value types are spelled out with
// their crate.
use std::sync::Arc;

use base::numerics::clamp_floor;
use computed_style::{
    ComputedStyle, LengthPercentageOrAuto, LengthPercentageOrNone, TableDisplay, TextDecorationLine,
};
use cssparser::{Parser, match_ignore_ascii_case};

use crate::animation::{
    ANIMATION, ANIMATION_RESET, SpecifiedEasing, SpecifiedTime, TRANSITION, parse_animation,
    parse_animation_duration, parse_animation_name, parse_animation_range,
    parse_animation_range_boundary, parse_animation_timeline, parse_easing, parse_fill_mode,
    parse_iteration_count, parse_play_state, parse_playback_direction, parse_time,
    parse_transition, parse_transition_behavior, parse_transition_property,
};
use crate::background::{
    SpecifiedBackgroundSize, SpecifiedImage, SpecifiedPosition, SpecifiedPositionComponent,
    parse_attachment, parse_box, parse_image, parse_list, parse_repeat,
};
use crate::css_url_data::UrlContext;
use crate::effects::{
    SpecifiedTransformOrigin, TransformFunction, compute_transform, compute_transform_origin,
    parse_transform, parse_transform_box, parse_transform_origin,
};
use crate::font::{
    FontSize as FontSizeValue, FontWeight as FontWeightValue, LineHeightValue, parse_font_family,
};
use crate::grid::{
    SpecifiedTrackList, areas_from_strings, compute_track_list, compute_track_sizes,
    parse_auto_flow, parse_explicit_track_list, parse_grid_line, parse_grid_line_and_name,
    parse_line_names, parse_template_areas, parse_track_list, parse_track_size, parse_track_sizes,
};
use crate::shadow::{SpecifiedShadow, compute_shadows, parse_shadows};
use crate::svg::{SpecifiedPaint, parse_d, parse_dasharray};
use crate::values::{
    ComputeContext, Length, LengthPercentage, LineWidth, ParseResult, SpecifiedColor,
    parse_integer, parse_number, parse_number_at_least, parse_style_color,
};
use crate::variables::{UnparsedValue, check_declaration_value, trim_value};
use computed_style::absolute_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LonghandId {
    Display,
    BoxSizing,
    Position,
    Float,
    Clear,
    Content,
    CounterReset,
    CounterIncrement,
    CounterSet,
    Top,
    Right,
    Bottom,
    Left,
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
    BorderTopLeftRadius,
    BorderTopRightRadius,
    BorderBottomRightRadius,
    BorderBottomLeftRadius,
    BoxShadow,
    OutlineColor,
    OutlineStyle,
    OutlineWidth,
    OutlineOffset,
    BackgroundColor,
    BackgroundImage,
    BackgroundPositionX,
    BackgroundPositionY,
    BackgroundSize,
    BackgroundRepeat,
    BackgroundAttachment,
    BackgroundClip,
    BackgroundOrigin,
    TextDecorationLine,
    TextDecorationStyle,
    TextDecorationColor,
    TextDecorationThickness,
    TextUnderlineOffset,
    TextUnderlinePosition,
    TextShadow,
    WebkitTextFillColor,
    Opacity,
    Filter,
    Isolation,
    OverflowX,
    OverflowY,
    Transform,
    TransformOrigin,
    TransformBox,
    ObjectFit,
    ObjectPosition,
    WebkitBoxOrient,
    WebkitBoxDirection,
    WebkitBoxPack,
    WebkitBoxAlign,
    WebkitBoxFlex,
    WebkitBoxOrdinalGroup,
    WebkitLineClamp,
    Contain,
    Clip,
    ClipPath,
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    Order,
    JustifyContent,
    AlignContent,
    AlignItems,
    AlignSelf,
    RowGap,
    ColumnGap,
    ZIndex,
    GridTemplateColumns,
    GridTemplateRows,
    GridAutoColumns,
    GridAutoRows,
    GridAutoFlow,
    GridTemplateAreas,
    GridRowStart,
    GridRowEnd,
    GridColumnStart,
    GridColumnEnd,
    JustifyItems,
    JustifySelf,
    Color,
    FontFamily,
    FontSize,
    FontWeight,
    FontStyle,
    FontStretch,
    FontVariantLigatures,
    FontVariantNumeric,
    FontVariantEastAsian,
    FontVariantCaps,
    FontVariantAlternates,
    FontVariantPosition,
    FontVariantEmoji,
    FontSizeAdjust,
    FontLanguageOverride,
    FontKerning,
    FontOpticalSizing,
    FontFeatureSettings,
    FontVariationSettings,
    FontSynthesisWeight,
    FontSynthesisStyle,
    FontSynthesisSmallCaps,
    LineHeight,
    TextAlign,
    VerticalAlign,
    WhiteSpaceCollapse,
    TextWrapMode,
    TextTransform,
    ListStyleType,
    ListStylePosition,
    ListStyleImage,
    TextOverflow,
    Appearance,
    LetterSpacing,
    WordSpacing,
    TabSize,
    TextIndent,
    WordBreak,
    OverflowWrap,
    Fill,
    FillOpacity,
    FillRule,
    ClipRule,
    Stroke,
    StrokeOpacity,
    StrokeWidth,
    StrokeLinecap,
    StrokeLinejoin,
    StrokeMiterlimit,
    StrokeDasharray,
    StrokeDashoffset,
    TextAnchor,
    StopColor,
    StopOpacity,
    Cx,
    Cy,
    R,
    Rx,
    Ry,
    X,
    Y,
    D,
    Visibility,
    WebkitTextSecurity,
    PointerEvents,
    BorderCollapse,
    WebkitBorderHorizontalSpacing,
    WebkitBorderVerticalSpacing,
    TableLayout,
    CaptionSide,
    EmptyCells,
    UserSelect,
    WebkitUserModify,
    Hyphens,
    ForcedColorAdjust,
    WebkitTapHighlightColor,
    CaretColor,
    TextSizeAdjust,
    TextRendering,
    WebkitFontSmoothing,
    Resize,
    ScrollBehavior,
    OverflowAnchor,
    Cursor,
    BorderImageSource,
    BorderImageSlice,
    BorderImageWidth,
    BorderImageOutset,
    BorderImageRepeat,
    TransitionProperty,
    TransitionDuration,
    TransitionTimingFunction,
    TransitionDelay,
    TransitionBehavior,
    AnimationDuration,
    AnimationTimingFunction,
    AnimationDelay,
    AnimationIterationCount,
    AnimationDirection,
    AnimationFillMode,
    AnimationPlayState,
    AnimationName,
    AnimationTimeline,
    AnimationRangeStart,
    AnimationRangeEnd,
    // The flow-relative (logical) properties of css-logical, each its own
    // property that the cascade maps to a physical one (`physical`).
    MarginBlockStart,
    MarginBlockEnd,
    MarginInlineStart,
    MarginInlineEnd,
    PaddingBlockStart,
    PaddingBlockEnd,
    PaddingInlineStart,
    PaddingInlineEnd,
    InsetBlockStart,
    InsetBlockEnd,
    InsetInlineStart,
    InsetInlineEnd,
    BorderBlockStartWidth,
    BorderBlockEndWidth,
    BorderInlineStartWidth,
    BorderInlineEndWidth,
    BorderBlockStartStyle,
    BorderBlockEndStyle,
    BorderInlineStartStyle,
    BorderInlineEndStyle,
    BorderBlockStartColor,
    BorderBlockEndColor,
    BorderInlineStartColor,
    BorderInlineEndColor,
    BorderStartStartRadius,
    BorderStartEndRadius,
    BorderEndStartRadius,
    BorderEndEndRadius,
    InlineSize,
    BlockSize,
    MinInlineSize,
    MinBlockSize,
    MaxInlineSize,
    MaxBlockSize,
}

use LonghandId::*;

/// The flow-relative longhands, as a pattern: the arms that map them to
/// their physical longhands first never see them.
macro_rules! logical_longhands {
    () => {
        MarginBlockStart
            | MarginBlockEnd
            | MarginInlineStart
            | MarginInlineEnd
            | PaddingBlockStart
            | PaddingBlockEnd
            | PaddingInlineStart
            | PaddingInlineEnd
            | InsetBlockStart
            | InsetBlockEnd
            | InsetInlineStart
            | InsetInlineEnd
            | BorderBlockStartWidth
            | BorderBlockEndWidth
            | BorderInlineStartWidth
            | BorderInlineEndWidth
            | BorderBlockStartStyle
            | BorderBlockEndStyle
            | BorderInlineStartStyle
            | BorderInlineEndStyle
            | BorderBlockStartColor
            | BorderBlockEndColor
            | BorderInlineStartColor
            | BorderInlineEndColor
            | BorderStartStartRadius
            | BorderStartEndRadius
            | BorderEndStartRadius
            | BorderEndEndRadius
            | InlineSize
            | BlockSize
            | MinInlineSize
            | MinBlockSize
            | MaxInlineSize
            | MaxBlockSize
    };
}
pub(crate) use logical_longhands;

const MARGIN: [LonghandId; 4] = [MarginTop, MarginRight, MarginBottom, MarginLeft];
const PADDING: [LonghandId; 4] = [PaddingTop, PaddingRight, PaddingBottom, PaddingLeft];
const INSET: [LonghandId; 4] = [Top, Right, Bottom, Left];
const BORDER_WIDTH: [LonghandId; 4] = [
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
];
const BORDER_STYLE: [LonghandId; 4] = [
    BorderTopStyle,
    BorderRightStyle,
    BorderBottomStyle,
    BorderLeftStyle,
];
const BORDER_COLOR: [LonghandId; 4] = [
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
];
/// In `border-radius` order: top left, top right, bottom right, bottom left.
/// `font-variant`'s longhands, in Chrome's order.
pub(crate) const FONT_VARIANT: [LonghandId; 7] = [
    FontVariantLigatures,
    FontVariantNumeric,
    FontVariantEastAsian,
    FontVariantCaps,
    FontVariantAlternates,
    FontVariantPosition,
    FontVariantEmoji,
];

/// `font`'s longhands, in Chrome's order (which `cssText` writes them in
/// when the shorthand can't be): it resets all of them but the
/// `font-synthesis-*` ones.
pub(crate) const FONT: [LonghandId; 19] = [
    FontStyle,
    FontVariantLigatures,
    FontVariantNumeric,
    FontVariantEastAsian,
    FontVariantCaps,
    FontVariantAlternates,
    FontVariantPosition,
    FontVariantEmoji,
    FontSizeAdjust,
    FontLanguageOverride,
    FontKerning,
    FontOpticalSizing,
    FontFeatureSettings,
    FontVariationSettings,
    FontWeight,
    FontStretch,
    FontSize,
    LineHeight,
    FontFamily,
];

/// `border-image`'s longhands, in its order.
pub(crate) const BORDER_IMAGE: [LonghandId; 5] = [
    BorderImageSource,
    BorderImageSlice,
    BorderImageWidth,
    BorderImageOutset,
    BorderImageRepeat,
];

pub(crate) const BORDER_RADIUS: [LonghandId; 4] = [
    BorderTopLeftRadius,
    BorderTopRightRadius,
    BorderBottomRightRadius,
    BorderBottomLeftRadius,
];
/// The flow-relative sides of a border, in `border-block-start`,
/// `-block-end`, `-inline-start`, `-inline-end` order.
const FLOW_BORDER_WIDTH: [LonghandId; 4] = [
    BorderBlockStartWidth,
    BorderBlockEndWidth,
    BorderInlineStartWidth,
    BorderInlineEndWidth,
];
const FLOW_BORDER_STYLE: [LonghandId; 4] = [
    BorderBlockStartStyle,
    BorderBlockEndStyle,
    BorderInlineStartStyle,
    BorderInlineEndStyle,
];
const FLOW_BORDER_COLOR: [LonghandId; 4] = [
    BorderBlockStartColor,
    BorderBlockEndColor,
    BorderInlineStartColor,
    BorderInlineEndColor,
];
const MARGIN_BLOCK: [LonghandId; 2] = [MarginBlockStart, MarginBlockEnd];
const MARGIN_INLINE: [LonghandId; 2] = [MarginInlineStart, MarginInlineEnd];
const PADDING_BLOCK: [LonghandId; 2] = [PaddingBlockStart, PaddingBlockEnd];
const PADDING_INLINE: [LonghandId; 2] = [PaddingInlineStart, PaddingInlineEnd];
const INSET_BLOCK: [LonghandId; 2] = [InsetBlockStart, InsetBlockEnd];
const INSET_INLINE: [LonghandId; 2] = [InsetInlineStart, InsetInlineEnd];
const BORDER_BLOCK_WIDTH: [LonghandId; 2] = [BorderBlockStartWidth, BorderBlockEndWidth];
const BORDER_BLOCK_STYLE: [LonghandId; 2] = [BorderBlockStartStyle, BorderBlockEndStyle];
const BORDER_BLOCK_COLOR: [LonghandId; 2] = [BorderBlockStartColor, BorderBlockEndColor];
const BORDER_INLINE_WIDTH: [LonghandId; 2] = [BorderInlineStartWidth, BorderInlineEndWidth];
const BORDER_INLINE_STYLE: [LonghandId; 2] = [BorderInlineStartStyle, BorderInlineEndStyle];
const BORDER_INLINE_COLOR: [LonghandId; 2] = [BorderInlineStartColor, BorderInlineEndColor];
/// The two-value flow-relative shorthands (`margin-block` and its kin):
/// each name and its start and end longhands.
const PAIRS: [(&str, [LonghandId; 2]); 12] = [
    ("margin-block", MARGIN_BLOCK),
    ("margin-inline", MARGIN_INLINE),
    ("padding-block", PADDING_BLOCK),
    ("padding-inline", PADDING_INLINE),
    ("inset-block", INSET_BLOCK),
    ("inset-inline", INSET_INLINE),
    ("border-block-width", BORDER_BLOCK_WIDTH),
    ("border-block-style", BORDER_BLOCK_STYLE),
    ("border-block-color", BORDER_BLOCK_COLOR),
    ("border-inline-width", BORDER_INLINE_WIDTH),
    ("border-inline-style", BORDER_INLINE_STYLE),
    ("border-inline-color", BORDER_INLINE_COLOR),
];
/// `background`'s longhands, in Blink's order.
pub(crate) const BACKGROUND: [LonghandId; 9] = [
    BackgroundImage,
    BackgroundPositionX,
    BackgroundPositionY,
    BackgroundSize,
    BackgroundRepeat,
    BackgroundAttachment,
    BackgroundOrigin,
    BackgroundClip,
    BackgroundColor,
];

impl LonghandId {
    /// How many longhands there are: [`Self::index`] is below it.
    pub const COUNT: usize = Self::ALL.len();

    /// A number for each longhand, from 0 to [`Self::COUNT`]: its place
    /// in [`Self::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Every longhand, in declaration order.
    pub const ALL: [Self; 249] = [
        Display,
        BoxSizing,
        Position,
        Float,
        Clear,
        Content,
        CounterReset,
        CounterIncrement,
        CounterSet,
        Top,
        Right,
        Bottom,
        Left,
        Width,
        Height,
        MinWidth,
        MinHeight,
        MaxWidth,
        MaxHeight,
        MarginTop,
        MarginRight,
        MarginBottom,
        MarginLeft,
        PaddingTop,
        PaddingRight,
        PaddingBottom,
        PaddingLeft,
        BorderTopWidth,
        BorderRightWidth,
        BorderBottomWidth,
        BorderLeftWidth,
        BorderTopStyle,
        BorderRightStyle,
        BorderBottomStyle,
        BorderLeftStyle,
        BorderTopColor,
        BorderRightColor,
        BorderBottomColor,
        BorderLeftColor,
        BorderTopLeftRadius,
        BorderTopRightRadius,
        BorderBottomRightRadius,
        BorderBottomLeftRadius,
        BoxShadow,
        OutlineColor,
        OutlineStyle,
        OutlineWidth,
        OutlineOffset,
        BackgroundColor,
        BackgroundImage,
        BackgroundPositionX,
        BackgroundPositionY,
        BackgroundSize,
        BackgroundRepeat,
        BackgroundAttachment,
        BackgroundClip,
        BackgroundOrigin,
        TextDecorationLine,
        TextDecorationStyle,
        TextDecorationColor,
        TextDecorationThickness,
        TextUnderlineOffset,
        TextUnderlinePosition,
        TextShadow,
        WebkitTextFillColor,
        Opacity,
        Filter,
        Isolation,
        OverflowX,
        OverflowY,
        Transform,
        TransformOrigin,
        TransformBox,
        ObjectFit,
        ObjectPosition,
        WebkitBoxOrient,
        WebkitBoxDirection,
        WebkitBoxPack,
        WebkitBoxAlign,
        WebkitBoxFlex,
        WebkitBoxOrdinalGroup,
        WebkitLineClamp,
        Contain,
        Clip,
        ClipPath,
        FlexDirection,
        FlexWrap,
        FlexGrow,
        FlexShrink,
        FlexBasis,
        Order,
        JustifyContent,
        AlignContent,
        AlignItems,
        AlignSelf,
        RowGap,
        ColumnGap,
        ZIndex,
        GridTemplateColumns,
        GridTemplateRows,
        GridAutoColumns,
        GridAutoRows,
        GridAutoFlow,
        GridTemplateAreas,
        GridRowStart,
        GridRowEnd,
        GridColumnStart,
        GridColumnEnd,
        JustifyItems,
        JustifySelf,
        Color,
        FontFamily,
        FontSize,
        FontWeight,
        FontStyle,
        FontStretch,
        FontVariantLigatures,
        FontVariantNumeric,
        FontVariantEastAsian,
        FontVariantCaps,
        FontVariantAlternates,
        FontVariantPosition,
        FontVariantEmoji,
        FontSizeAdjust,
        FontLanguageOverride,
        FontKerning,
        FontOpticalSizing,
        FontFeatureSettings,
        FontVariationSettings,
        FontSynthesisWeight,
        FontSynthesisStyle,
        FontSynthesisSmallCaps,
        LineHeight,
        TextAlign,
        VerticalAlign,
        WhiteSpaceCollapse,
        TextWrapMode,
        TextTransform,
        ListStyleType,
        ListStylePosition,
        ListStyleImage,
        TextOverflow,
        Appearance,
        LetterSpacing,
        WordSpacing,
        TabSize,
        TextIndent,
        WordBreak,
        OverflowWrap,
        Fill,
        FillOpacity,
        FillRule,
        ClipRule,
        Stroke,
        StrokeOpacity,
        StrokeWidth,
        StrokeLinecap,
        StrokeLinejoin,
        StrokeMiterlimit,
        StrokeDasharray,
        StrokeDashoffset,
        TextAnchor,
        StopColor,
        StopOpacity,
        Cx,
        Cy,
        R,
        Rx,
        Ry,
        X,
        Y,
        D,
        Visibility,
        WebkitTextSecurity,
        PointerEvents,
        BorderCollapse,
        WebkitBorderHorizontalSpacing,
        WebkitBorderVerticalSpacing,
        TableLayout,
        CaptionSide,
        EmptyCells,
        UserSelect,
        WebkitUserModify,
        Hyphens,
        ForcedColorAdjust,
        WebkitTapHighlightColor,
        CaretColor,
        TextSizeAdjust,
        TextRendering,
        WebkitFontSmoothing,
        Resize,
        ScrollBehavior,
        OverflowAnchor,
        Cursor,
        BorderImageSource,
        BorderImageSlice,
        BorderImageWidth,
        BorderImageOutset,
        BorderImageRepeat,
        TransitionProperty,
        TransitionDuration,
        TransitionTimingFunction,
        TransitionDelay,
        TransitionBehavior,
        AnimationDuration,
        AnimationTimingFunction,
        AnimationDelay,
        AnimationIterationCount,
        AnimationDirection,
        AnimationFillMode,
        AnimationPlayState,
        AnimationName,
        AnimationTimeline,
        AnimationRangeStart,
        AnimationRangeEnd,
        MarginBlockStart,
        MarginBlockEnd,
        MarginInlineStart,
        MarginInlineEnd,
        PaddingBlockStart,
        PaddingBlockEnd,
        PaddingInlineStart,
        PaddingInlineEnd,
        InsetBlockStart,
        InsetBlockEnd,
        InsetInlineStart,
        InsetInlineEnd,
        BorderBlockStartWidth,
        BorderBlockEndWidth,
        BorderInlineStartWidth,
        BorderInlineEndWidth,
        BorderBlockStartStyle,
        BorderBlockEndStyle,
        BorderInlineStartStyle,
        BorderInlineEndStyle,
        BorderBlockStartColor,
        BorderBlockEndColor,
        BorderInlineStartColor,
        BorderInlineEndColor,
        BorderStartStartRadius,
        BorderStartEndRadius,
        BorderEndStartRadius,
        BorderEndEndRadius,
        InlineSize,
        BlockSize,
        MinInlineSize,
        MinBlockSize,
        MaxInlineSize,
        MaxBlockSize,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match_ignore_ascii_case! { name,
            "display" => Display,
            "box-sizing" => BoxSizing,
            "position" => Position,
            "float" => Float,
            "clear" => Clear,
            "content" => Content,
            "counter-reset" => CounterReset,
            "counter-increment" => CounterIncrement,
            "counter-set" => CounterSet,
            "top" => Top,
            "right" => Right,
            "bottom" => Bottom,
            "left" => Left,
            "width" => Width,
            "height" => Height,
            "min-width" => MinWidth,
            "min-height" => MinHeight,
            "max-width" => MaxWidth,
            "max-height" => MaxHeight,
            "margin-top" => MarginTop,
            "margin-right" => MarginRight,
            "margin-bottom" => MarginBottom,
            "margin-left" => MarginLeft,
            "padding-top" => PaddingTop,
            "padding-right" => PaddingRight,
            "padding-bottom" => PaddingBottom,
            "padding-left" => PaddingLeft,
            "border-top-width" => BorderTopWidth,
            "border-right-width" => BorderRightWidth,
            "border-bottom-width" => BorderBottomWidth,
            "border-left-width" => BorderLeftWidth,
            "border-top-style" => BorderTopStyle,
            "border-right-style" => BorderRightStyle,
            "border-bottom-style" => BorderBottomStyle,
            "border-left-style" => BorderLeftStyle,
            "border-top-color" => BorderTopColor,
            "border-right-color" => BorderRightColor,
            "border-bottom-color" => BorderBottomColor,
            "border-left-color" => BorderLeftColor,
            "border-top-left-radius" => BorderTopLeftRadius,
            "border-top-right-radius" => BorderTopRightRadius,
            "border-bottom-right-radius" => BorderBottomRightRadius,
            "border-bottom-left-radius" => BorderBottomLeftRadius,
            "box-shadow" => BoxShadow,
            "outline-color" => OutlineColor,
            "outline-style" => OutlineStyle,
            "outline-width" => OutlineWidth,
            "outline-offset" => OutlineOffset,
            "background-color" => BackgroundColor,
            "background-image" => BackgroundImage,
            "background-position-x" => BackgroundPositionX,
            "background-position-y" => BackgroundPositionY,
            "background-size" => BackgroundSize,
            "background-repeat" => BackgroundRepeat,
            "background-attachment" => BackgroundAttachment,
            "background-clip" => BackgroundClip,
            "background-origin" => BackgroundOrigin,
            "text-decoration-line" => TextDecorationLine,
            "text-decoration-style" => TextDecorationStyle,
            "text-decoration-color" => TextDecorationColor,
            "text-decoration-thickness" => TextDecorationThickness,
            "text-underline-offset" => TextUnderlineOffset,
            "text-underline-position" => TextUnderlinePosition,
            "text-shadow" => TextShadow,
            "-webkit-text-fill-color" => WebkitTextFillColor,
            "filter" => Filter,
            "isolation" => Isolation,
            "opacity" => Opacity,
            "overflow-x" => OverflowX,
            "overflow-y" => OverflowY,
            "transform" | "-webkit-transform" => Transform,
            "transform-origin" | "-webkit-transform-origin" => TransformOrigin,
            "transform-box" => TransformBox,
            "object-fit" => ObjectFit,
            "object-position" => ObjectPosition,
            "-webkit-box-orient" => WebkitBoxOrient,
            "-webkit-box-direction" => WebkitBoxDirection,
            "-webkit-box-pack" => WebkitBoxPack,
            "-webkit-box-align" => WebkitBoxAlign,
            "-webkit-box-flex" => WebkitBoxFlex,
            "-webkit-box-ordinal-group" => WebkitBoxOrdinalGroup,
            "-webkit-line-clamp" => WebkitLineClamp,
            "contain" => Contain,
            "clip" => Clip,
            "clip-path" => ClipPath,
            "flex-direction" => FlexDirection,
            "flex-wrap" => FlexWrap,
            "flex-grow" => FlexGrow,
            "flex-shrink" => FlexShrink,
            "flex-basis" => FlexBasis,
            "order" => Order,
            "justify-content" => JustifyContent,
            "align-content" => AlignContent,
            "align-items" => AlignItems,
            "align-self" => AlignSelf,
            // `grid-row-gap` and `grid-column-gap` are their legacy names.
            "row-gap" | "grid-row-gap" => RowGap,
            "column-gap" | "grid-column-gap" => ColumnGap,
            "grid-template-columns" => GridTemplateColumns,
            "grid-template-rows" => GridTemplateRows,
            "grid-auto-columns" => GridAutoColumns,
            "grid-auto-rows" => GridAutoRows,
            "grid-auto-flow" => GridAutoFlow,
            "grid-template-areas" => GridTemplateAreas,
            "grid-row-start" => GridRowStart,
            "grid-row-end" => GridRowEnd,
            "grid-column-start" => GridColumnStart,
            "grid-column-end" => GridColumnEnd,
            "justify-items" => JustifyItems,
            "justify-self" => JustifySelf,
            "z-index" => ZIndex,
            "color" => Color,
            "font-family" => FontFamily,
            "font-size" => FontSize,
            "font-weight" => FontWeight,
            "font-style" => FontStyle,
            "font-stretch" => FontStretch,
            "font-variant-ligatures" => FontVariantLigatures,
            "font-variant-numeric" => FontVariantNumeric,
            "font-variant-east-asian" => FontVariantEastAsian,
            "font-variant-caps" => FontVariantCaps,
            "font-variant-alternates" => FontVariantAlternates,
            "font-variant-position" => FontVariantPosition,
            "font-variant-emoji" => FontVariantEmoji,
            "font-size-adjust" => FontSizeAdjust,
            "font-language-override" => FontLanguageOverride,
            "font-kerning" => FontKerning,
            "font-optical-sizing" => FontOpticalSizing,
            "font-feature-settings" => FontFeatureSettings,
            "font-variation-settings" => FontVariationSettings,
            "font-synthesis-weight" => FontSynthesisWeight,
            "font-synthesis-style" => FontSynthesisStyle,
            "font-synthesis-small-caps" => FontSynthesisSmallCaps,
            "line-height" => LineHeight,
            "text-align" => TextAlign,
            "vertical-align" => VerticalAlign,
            "white-space-collapse" => WhiteSpaceCollapse,
            "text-wrap-mode" => TextWrapMode,
            "text-transform" => TextTransform,
            "list-style-type" => ListStyleType,
            "list-style-position" => ListStylePosition,
            "list-style-image" => ListStyleImage,
            "text-overflow" => TextOverflow,
            // Chrome's alias, which reads back as `appearance`.
            "appearance" | "-webkit-appearance" => Appearance,
            "letter-spacing" => LetterSpacing,
            "word-spacing" => WordSpacing,
            "tab-size" => TabSize,
            "text-indent" => TextIndent,
            "word-break" => WordBreak,
            "overflow-wrap" | "word-wrap" => OverflowWrap,
            "fill" => Fill,
            "fill-opacity" => FillOpacity,
            "fill-rule" => FillRule,
            "clip-rule" => ClipRule,
            "stroke" => Stroke,
            "stroke-opacity" => StrokeOpacity,
            "stroke-width" => StrokeWidth,
            "stroke-linecap" => StrokeLinecap,
            "stroke-linejoin" => StrokeLinejoin,
            "stroke-miterlimit" => StrokeMiterlimit,
            "stroke-dasharray" => StrokeDasharray,
            "stroke-dashoffset" => StrokeDashoffset,
            "text-anchor" => TextAnchor,
            "stop-color" => StopColor,
            "stop-opacity" => StopOpacity,
            "cx" => Cx,
            "cy" => Cy,
            "r" => R,
            "rx" => Rx,
            "ry" => Ry,
            "x" => X,
            "y" => Y,
            "d" => D,
            "visibility" => Visibility,
            "-webkit-text-security" => WebkitTextSecurity,
            "pointer-events" => PointerEvents,
            "border-collapse" => BorderCollapse,
            "-webkit-border-horizontal-spacing" => WebkitBorderHorizontalSpacing,
            "-webkit-border-vertical-spacing" => WebkitBorderVerticalSpacing,
            "table-layout" => TableLayout,
            "caption-side" => CaptionSide,
            "empty-cells" => EmptyCells,
            "user-select" | "-webkit-user-select" => UserSelect,
            "-webkit-user-modify" => WebkitUserModify,
            "hyphens" => Hyphens,
            "forced-color-adjust" => ForcedColorAdjust,
            "-webkit-tap-highlight-color" => WebkitTapHighlightColor,
            "caret-color" => CaretColor,
            "text-size-adjust" | "-webkit-text-size-adjust" => TextSizeAdjust,
            "text-rendering" => TextRendering,
            "-webkit-font-smoothing" => WebkitFontSmoothing,
            "resize" => Resize,
            "scroll-behavior" => ScrollBehavior,
            "overflow-anchor" => OverflowAnchor,
            "cursor" => Cursor,
            "border-image-source" => BorderImageSource,
            "border-image-slice" => BorderImageSlice,
            "border-image-width" => BorderImageWidth,
            "border-image-outset" => BorderImageOutset,
            "border-image-repeat" => BorderImageRepeat,
            "transition-property" => TransitionProperty,
            "transition-duration" => TransitionDuration,
            "transition-timing-function" => TransitionTimingFunction,
            "transition-delay" => TransitionDelay,
            "transition-behavior" => TransitionBehavior,
            "animation-duration" => AnimationDuration,
            "animation-timing-function" => AnimationTimingFunction,
            "animation-delay" => AnimationDelay,
            "animation-iteration-count" => AnimationIterationCount,
            "animation-direction" => AnimationDirection,
            "animation-fill-mode" => AnimationFillMode,
            "animation-play-state" => AnimationPlayState,
            "animation-name" => AnimationName,
            "animation-timeline" => AnimationTimeline,
            "animation-range-start" => AnimationRangeStart,
            "animation-range-end" => AnimationRangeEnd,
            "margin-block-start" => MarginBlockStart,
            "margin-block-end" => MarginBlockEnd,
            "margin-inline-start" => MarginInlineStart,
            "margin-inline-end" => MarginInlineEnd,
            "padding-block-start" => PaddingBlockStart,
            "padding-block-end" => PaddingBlockEnd,
            "padding-inline-start" => PaddingInlineStart,
            "padding-inline-end" => PaddingInlineEnd,
            "inset-block-start" => InsetBlockStart,
            "inset-block-end" => InsetBlockEnd,
            "inset-inline-start" => InsetInlineStart,
            "inset-inline-end" => InsetInlineEnd,
            "border-block-start-width" => BorderBlockStartWidth,
            "border-block-end-width" => BorderBlockEndWidth,
            "border-inline-start-width" => BorderInlineStartWidth,
            "border-inline-end-width" => BorderInlineEndWidth,
            "border-block-start-style" => BorderBlockStartStyle,
            "border-block-end-style" => BorderBlockEndStyle,
            "border-inline-start-style" => BorderInlineStartStyle,
            "border-inline-end-style" => BorderInlineEndStyle,
            "border-block-start-color" => BorderBlockStartColor,
            "border-block-end-color" => BorderBlockEndColor,
            "border-inline-start-color" => BorderInlineStartColor,
            "border-inline-end-color" => BorderInlineEndColor,
            "border-start-start-radius" => BorderStartStartRadius,
            "border-start-end-radius" => BorderStartEndRadius,
            "border-end-start-radius" => BorderEndStartRadius,
            "border-end-end-radius" => BorderEndEndRadius,
            "inline-size" => InlineSize,
            "block-size" => BlockSize,
            "min-inline-size" => MinInlineSize,
            "min-block-size" => MinBlockSize,
            "max-inline-size" => MaxInlineSize,
            "max-block-size" => MaxBlockSize,
            _ => return None,
        })
    }

    /// The property's name (for an alias, the standard one).
    pub fn name(self) -> &'static str {
        match self {
            Display => "display",
            Position => "position",
            Float => "float",
            Clear => "clear",
            Content => "content",
            CounterReset => "counter-reset",
            CounterIncrement => "counter-increment",
            CounterSet => "counter-set",
            Top => "top",
            Right => "right",
            Bottom => "bottom",
            Left => "left",
            BoxSizing => "box-sizing",
            Width => "width",
            Height => "height",
            MinWidth => "min-width",
            MinHeight => "min-height",
            MaxWidth => "max-width",
            MaxHeight => "max-height",
            MarginTop => "margin-top",
            MarginRight => "margin-right",
            MarginBottom => "margin-bottom",
            MarginLeft => "margin-left",
            PaddingTop => "padding-top",
            PaddingRight => "padding-right",
            PaddingBottom => "padding-bottom",
            PaddingLeft => "padding-left",
            BorderTopWidth => "border-top-width",
            BorderRightWidth => "border-right-width",
            BorderBottomWidth => "border-bottom-width",
            BorderLeftWidth => "border-left-width",
            BorderTopStyle => "border-top-style",
            BorderRightStyle => "border-right-style",
            BorderBottomStyle => "border-bottom-style",
            BorderLeftStyle => "border-left-style",
            BorderTopColor => "border-top-color",
            BorderRightColor => "border-right-color",
            BorderBottomColor => "border-bottom-color",
            BorderLeftColor => "border-left-color",
            BorderTopLeftRadius => "border-top-left-radius",
            BorderTopRightRadius => "border-top-right-radius",
            BorderBottomRightRadius => "border-bottom-right-radius",
            BorderBottomLeftRadius => "border-bottom-left-radius",
            BoxShadow => "box-shadow",
            OutlineColor => "outline-color",
            OutlineStyle => "outline-style",
            OutlineWidth => "outline-width",
            OutlineOffset => "outline-offset",
            BackgroundColor => "background-color",
            BackgroundImage => "background-image",
            BackgroundPositionX => "background-position-x",
            BackgroundPositionY => "background-position-y",
            BackgroundSize => "background-size",
            BackgroundRepeat => "background-repeat",
            BackgroundAttachment => "background-attachment",
            BackgroundClip => "background-clip",
            BackgroundOrigin => "background-origin",
            TextDecorationLine => "text-decoration-line",
            TextDecorationStyle => "text-decoration-style",
            TextDecorationColor => "text-decoration-color",
            TextDecorationThickness => "text-decoration-thickness",
            TextUnderlineOffset => "text-underline-offset",
            TextUnderlinePosition => "text-underline-position",
            TextShadow => "text-shadow",
            WebkitTextFillColor => "-webkit-text-fill-color",
            Filter => "filter",
            Isolation => "isolation",
            Opacity => "opacity",
            OverflowX => "overflow-x",
            OverflowY => "overflow-y",
            Transform => "transform",
            TransformOrigin => "transform-origin",
            TransformBox => "transform-box",
            ObjectFit => "object-fit",
            ObjectPosition => "object-position",
            WebkitBoxOrient => "-webkit-box-orient",
            WebkitBoxDirection => "-webkit-box-direction",
            WebkitBoxPack => "-webkit-box-pack",
            WebkitBoxAlign => "-webkit-box-align",
            WebkitBoxFlex => "-webkit-box-flex",
            WebkitBoxOrdinalGroup => "-webkit-box-ordinal-group",
            WebkitLineClamp => "-webkit-line-clamp",
            Contain => "contain",
            Clip => "clip",
            ClipPath => "clip-path",
            FlexDirection => "flex-direction",
            FlexWrap => "flex-wrap",
            FlexGrow => "flex-grow",
            FlexShrink => "flex-shrink",
            FlexBasis => "flex-basis",
            Order => "order",
            JustifyContent => "justify-content",
            AlignContent => "align-content",
            AlignItems => "align-items",
            AlignSelf => "align-self",
            RowGap => "row-gap",
            ColumnGap => "column-gap",
            GridTemplateColumns => "grid-template-columns",
            GridTemplateRows => "grid-template-rows",
            GridAutoColumns => "grid-auto-columns",
            GridAutoRows => "grid-auto-rows",
            GridAutoFlow => "grid-auto-flow",
            GridTemplateAreas => "grid-template-areas",
            GridRowStart => "grid-row-start",
            GridRowEnd => "grid-row-end",
            GridColumnStart => "grid-column-start",
            GridColumnEnd => "grid-column-end",
            JustifyItems => "justify-items",
            JustifySelf => "justify-self",
            ZIndex => "z-index",
            Color => "color",
            FontFamily => "font-family",
            FontSize => "font-size",
            FontWeight => "font-weight",
            FontStyle => "font-style",
            FontStretch => "font-stretch",
            FontVariantLigatures => "font-variant-ligatures",
            FontVariantNumeric => "font-variant-numeric",
            FontVariantEastAsian => "font-variant-east-asian",
            FontVariantCaps => "font-variant-caps",
            FontVariantAlternates => "font-variant-alternates",
            FontVariantPosition => "font-variant-position",
            FontVariantEmoji => "font-variant-emoji",
            FontSizeAdjust => "font-size-adjust",
            FontLanguageOverride => "font-language-override",
            FontKerning => "font-kerning",
            FontOpticalSizing => "font-optical-sizing",
            FontFeatureSettings => "font-feature-settings",
            FontVariationSettings => "font-variation-settings",
            FontSynthesisWeight => "font-synthesis-weight",
            FontSynthesisStyle => "font-synthesis-style",
            FontSynthesisSmallCaps => "font-synthesis-small-caps",
            LineHeight => "line-height",
            TextAlign => "text-align",
            VerticalAlign => "vertical-align",
            WhiteSpaceCollapse => "white-space-collapse",
            TextWrapMode => "text-wrap-mode",
            TextTransform => "text-transform",
            ListStyleType => "list-style-type",
            ListStylePosition => "list-style-position",
            ListStyleImage => "list-style-image",
            TextOverflow => "text-overflow",
            Appearance => "appearance",
            LetterSpacing => "letter-spacing",
            WordSpacing => "word-spacing",
            TabSize => "tab-size",
            TextIndent => "text-indent",
            WordBreak => "word-break",
            OverflowWrap => "overflow-wrap",
            Fill => "fill",
            FillOpacity => "fill-opacity",
            FillRule => "fill-rule",
            ClipRule => "clip-rule",
            Stroke => "stroke",
            StrokeOpacity => "stroke-opacity",
            StrokeWidth => "stroke-width",
            StrokeLinecap => "stroke-linecap",
            StrokeLinejoin => "stroke-linejoin",
            StrokeMiterlimit => "stroke-miterlimit",
            StrokeDasharray => "stroke-dasharray",
            StrokeDashoffset => "stroke-dashoffset",
            TextAnchor => "text-anchor",
            StopColor => "stop-color",
            StopOpacity => "stop-opacity",
            Cx => "cx",
            Cy => "cy",
            R => "r",
            Rx => "rx",
            Ry => "ry",
            X => "x",
            Y => "y",
            D => "d",
            Visibility => "visibility",
            WebkitTextSecurity => "-webkit-text-security",
            PointerEvents => "pointer-events",
            BorderCollapse => "border-collapse",
            WebkitBorderHorizontalSpacing => "-webkit-border-horizontal-spacing",
            WebkitBorderVerticalSpacing => "-webkit-border-vertical-spacing",
            TableLayout => "table-layout",
            CaptionSide => "caption-side",
            EmptyCells => "empty-cells",
            UserSelect => "user-select",
            WebkitUserModify => "-webkit-user-modify",
            Hyphens => "hyphens",
            ForcedColorAdjust => "forced-color-adjust",
            WebkitTapHighlightColor => "-webkit-tap-highlight-color",
            CaretColor => "caret-color",
            TextSizeAdjust => "text-size-adjust",
            TextRendering => "text-rendering",
            WebkitFontSmoothing => "-webkit-font-smoothing",
            Resize => "resize",
            ScrollBehavior => "scroll-behavior",
            OverflowAnchor => "overflow-anchor",
            Cursor => "cursor",
            BorderImageSource => "border-image-source",
            BorderImageSlice => "border-image-slice",
            BorderImageWidth => "border-image-width",
            BorderImageOutset => "border-image-outset",
            BorderImageRepeat => "border-image-repeat",
            TransitionProperty => "transition-property",
            TransitionDuration => "transition-duration",
            TransitionTimingFunction => "transition-timing-function",
            TransitionDelay => "transition-delay",
            TransitionBehavior => "transition-behavior",
            AnimationDuration => "animation-duration",
            AnimationTimingFunction => "animation-timing-function",
            AnimationDelay => "animation-delay",
            AnimationIterationCount => "animation-iteration-count",
            AnimationDirection => "animation-direction",
            AnimationFillMode => "animation-fill-mode",
            AnimationPlayState => "animation-play-state",
            AnimationName => "animation-name",
            AnimationTimeline => "animation-timeline",
            AnimationRangeStart => "animation-range-start",
            AnimationRangeEnd => "animation-range-end",
            MarginBlockStart => "margin-block-start",
            MarginBlockEnd => "margin-block-end",
            MarginInlineStart => "margin-inline-start",
            MarginInlineEnd => "margin-inline-end",
            PaddingBlockStart => "padding-block-start",
            PaddingBlockEnd => "padding-block-end",
            PaddingInlineStart => "padding-inline-start",
            PaddingInlineEnd => "padding-inline-end",
            InsetBlockStart => "inset-block-start",
            InsetBlockEnd => "inset-block-end",
            InsetInlineStart => "inset-inline-start",
            InsetInlineEnd => "inset-inline-end",
            BorderBlockStartWidth => "border-block-start-width",
            BorderBlockEndWidth => "border-block-end-width",
            BorderInlineStartWidth => "border-inline-start-width",
            BorderInlineEndWidth => "border-inline-end-width",
            BorderBlockStartStyle => "border-block-start-style",
            BorderBlockEndStyle => "border-block-end-style",
            BorderInlineStartStyle => "border-inline-start-style",
            BorderInlineEndStyle => "border-inline-end-style",
            BorderBlockStartColor => "border-block-start-color",
            BorderBlockEndColor => "border-block-end-color",
            BorderInlineStartColor => "border-inline-start-color",
            BorderInlineEndColor => "border-inline-end-color",
            BorderStartStartRadius => "border-start-start-radius",
            BorderStartEndRadius => "border-start-end-radius",
            BorderEndStartRadius => "border-end-start-radius",
            BorderEndEndRadius => "border-end-end-radius",
            InlineSize => "inline-size",
            BlockSize => "block-size",
            MinInlineSize => "min-inline-size",
            MinBlockSize => "min-block-size",
            MaxInlineSize => "max-inline-size",
            MaxBlockSize => "max-block-size",
        }
    }

    /// The physical longhand a flow-relative one maps to, and itself for
    /// any other. machopo has only `writing-mode: horizontal-tb` and
    /// `direction: ltr`, so block-start is top, inline-start is left (a
    /// corner's block side first: start-end is the top right) and the
    /// inline size is the width (Blink's `CSSDirectionAwareResolver`
    /// for those).
    pub fn physical(self) -> Self {
        match self {
            MarginBlockStart => MarginTop,
            MarginBlockEnd => MarginBottom,
            MarginInlineStart => MarginLeft,
            MarginInlineEnd => MarginRight,
            PaddingBlockStart => PaddingTop,
            PaddingBlockEnd => PaddingBottom,
            PaddingInlineStart => PaddingLeft,
            PaddingInlineEnd => PaddingRight,
            InsetBlockStart => Top,
            InsetBlockEnd => Bottom,
            InsetInlineStart => Left,
            InsetInlineEnd => Right,
            BorderBlockStartWidth => BorderTopWidth,
            BorderBlockEndWidth => BorderBottomWidth,
            BorderInlineStartWidth => BorderLeftWidth,
            BorderInlineEndWidth => BorderRightWidth,
            BorderBlockStartStyle => BorderTopStyle,
            BorderBlockEndStyle => BorderBottomStyle,
            BorderInlineStartStyle => BorderLeftStyle,
            BorderInlineEndStyle => BorderRightStyle,
            BorderBlockStartColor => BorderTopColor,
            BorderBlockEndColor => BorderBottomColor,
            BorderInlineStartColor => BorderLeftColor,
            BorderInlineEndColor => BorderRightColor,
            BorderStartStartRadius => BorderTopLeftRadius,
            BorderStartEndRadius => BorderTopRightRadius,
            BorderEndStartRadius => BorderBottomLeftRadius,
            BorderEndEndRadius => BorderBottomRightRadius,
            InlineSize => Width,
            BlockSize => Height,
            MinInlineSize => MinWidth,
            MinBlockSize => MinHeight,
            MaxInlineSize => MaxWidth,
            MaxBlockSize => MaxHeight,
            _ => self,
        }
    }

    /// Whether this is a flow-relative longhand.
    pub fn is_logical(self) -> bool {
        self.physical() != self
    }

    /// The logical property group this longhand is in (css-logical), as
    /// the physical longhands of the group: a flow-relative longhand and
    /// its physical ones, which the cascade weighs against each other in
    /// declaration order.
    fn logical_group(self) -> Option<&'static [LonghandId]> {
        const SIZE: [LonghandId; 2] = [Width, Height];
        const MIN_SIZE: [LonghandId; 2] = [MinWidth, MinHeight];
        const MAX_SIZE: [LonghandId; 2] = [MaxWidth, MaxHeight];
        let physical = self.physical();
        [
            &MARGIN[..],
            &PADDING,
            &INSET,
            &BORDER_WIDTH,
            &BORDER_STYLE,
            &BORDER_COLOR,
            &BORDER_RADIUS,
            &SIZE,
            &MIN_SIZE,
            &MAX_SIZE,
        ]
        .into_iter()
        .find(|group| group.contains(&physical))
    }

    /// Whether `other` is in the same logical property group with the
    /// other mapping logic (one flow-relative, the other physical; CSSOM's
    /// rule for where `setProperty` puts a declaration, Blink's
    /// `IsInSameLogicalPropertyGroupWithDifferentMappingLogic`).
    pub(crate) fn is_in_same_logical_group_with_other_mapping(self, other: Self) -> bool {
        self.is_logical() != other.is_logical()
            && self
                .logical_group()
                .is_some_and(|group| other.logical_group() == Some(group))
    }

    /// Whether it is one of `background`'s longhands: an author's
    /// declaration of one draws a button or a text field as CSS does
    /// (Blink's `ComputedStyle::HasAuthorBackground`).
    pub fn is_background(self) -> bool {
        BACKGROUND.contains(&self)
    }

    /// Whether it is a physical side's border width, style or color (Blink's
    /// `HasAuthorBorder`).
    pub fn is_border(self) -> bool {
        BORDER_WIDTH.contains(&self) || BORDER_STYLE.contains(&self) || BORDER_COLOR.contains(&self)
    }

    /// Whether it is a corner's `border-*-radius` (Blink's
    /// `HasAuthorBorderRadius`).
    pub fn is_border_radius(self) -> bool {
        BORDER_RADIUS.contains(&self)
    }

    /// Whether it applies to `::marker` (css-pseudo 4, "List Markers":
    /// the font properties, `white-space`, `color`, `content`, the text
    /// properties machopo has (`text-shadow` too, not the decorations), and
    /// the animation and transition properties;
    /// Blink's `kValidForMarker`): the others'
    /// declarations for a marker are ignored, as the list properties',
    /// which are the list item's.
    pub fn applies_to_marker(self) -> bool {
        matches!(
            self,
            Color
                | FontFamily
                | FontSize
                | FontWeight
                | FontStyle
                | FontStretch
                | FontVariantLigatures
                | FontVariantNumeric
                | FontVariantEastAsian
                | FontVariantCaps
                | FontVariantAlternates
                | FontVariantPosition
                | FontVariantEmoji
                | FontSizeAdjust
                | FontLanguageOverride
                | FontKerning
                | FontOpticalSizing
                | FontFeatureSettings
                | FontVariationSettings
                | FontSynthesisWeight
                | FontSynthesisStyle
                | FontSynthesisSmallCaps
                | LineHeight
                | WhiteSpaceCollapse
                | TextWrapMode
                | TextTransform
                | LetterSpacing
                | WordSpacing
                | TabSize
                | WordBreak
                | OverflowWrap
                | TextShadow
                | Content
                | TransitionProperty
                | TransitionDuration
                | TransitionTimingFunction
                | TransitionDelay
                | TransitionBehavior
                | AnimationDuration
                | AnimationTimingFunction
                | AnimationDelay
                | AnimationIterationCount
                | AnimationDirection
                | AnimationFillMode
                | AnimationPlayState
                | AnimationName
                | AnimationTimeline
                | AnimationRangeStart
                | AnimationRangeEnd
        )
    }

    /// Whether it applies to a highlight pseudo-element (`::selection`):
    /// those of Blink's `css_properties.json5` marked
    /// `valid_for_highlight` that machopo has (css-pseudo 4, "Styling
    /// Highlights"). The others' declarations are ignored, and read back
    /// as the originating element's.
    pub fn applies_to_highlight(self) -> bool {
        matches!(
            self,
            Color
                | BackgroundColor
                | Fill
                | Stroke
                | StrokeWidth
                | TextDecorationLine
                | TextDecorationStyle
                | TextDecorationColor
                | TextDecorationThickness
                | TextUnderlineOffset
                | TextShadow
        )
    }

    pub fn is_inherited(self) -> bool {
        matches!(
            self,
            Color
                | FontFamily
                | FontSize
                | FontWeight
                | FontStyle
                | FontStretch
                | FontVariantLigatures
                | FontVariantNumeric
                | FontVariantEastAsian
                | FontVariantCaps
                | FontVariantAlternates
                | FontVariantPosition
                | FontVariantEmoji
                | FontSizeAdjust
                | FontLanguageOverride
                | FontKerning
                | FontOpticalSizing
                | FontFeatureSettings
                | FontVariationSettings
                | FontSynthesisWeight
                | FontSynthesisStyle
                | FontSynthesisSmallCaps
                | LineHeight
                | TextAlign
                | WhiteSpaceCollapse
                | TextWrapMode
                | TextTransform
                | ListStyleType
                | ListStylePosition
                | ListStyleImage
                | LetterSpacing
                | WordSpacing
                | TabSize
                | TextIndent
                | WordBreak
                | OverflowWrap
                | TextUnderlineOffset
                | TextUnderlinePosition
                | TextShadow
                | WebkitTextFillColor
                | Fill
                | FillOpacity
                | FillRule
                | ClipRule
                | Stroke
                | StrokeOpacity
                | StrokeWidth
                | StrokeLinecap
                | StrokeLinejoin
                | StrokeMiterlimit
                | StrokeDasharray
                | StrokeDashoffset
                | TextAnchor
                | Visibility
                | WebkitTextSecurity
                | PointerEvents
                | BorderCollapse
                | WebkitBorderHorizontalSpacing
                | WebkitBorderVerticalSpacing
                | CaptionSide
                | EmptyCells
                | UserSelect
                | WebkitUserModify
                | Hyphens
                | ForcedColorAdjust
                | WebkitTapHighlightColor
                | CaretColor
                | TextSizeAdjust
                | TextRendering
                | WebkitFontSmoothing
                | Cursor
        )
    }

    /// Parses a value (not a CSS-wide keyword) for this property.
    fn parse_value<'i>(self, input: &mut Parser<'i, '_>) -> ParseResult<'i, Value> {
        Ok(match self.physical() {
            logical_longhands!() => unreachable!("mapped to a physical longhand"),
            Display => Value::Display(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "block" => computed_style::Display::Block,
                    "inline" => computed_style::Display::Inline,
                    "flex" => computed_style::Display::Flex,
                    "grid" => computed_style::Display::Grid,
                    "inline-block" => computed_style::Display::InlineBlock,
                    "inline-flex" => computed_style::Display::InlineFlex,
                    "inline-grid" => computed_style::Display::InlineGrid,
                    "flow-root" => computed_style::Display::FlowRoot,
                    "-webkit-box" => computed_style::Display::WebkitBox,
                    "-webkit-inline-box" => computed_style::Display::WebkitInlineBox,
                    // Laid out as `block` and `inline-block` until there
                    // are tables (`Display::Table`).
                    "table" => computed_style::Display::Table(TableDisplay::Table),
                    "inline-table" => computed_style::Display::InlineTable,
                    "table-row-group" => computed_style::Display::Table(TableDisplay::RowGroup),
                    "table-header-group" => computed_style::Display::Table(TableDisplay::HeaderGroup),
                    "table-footer-group" => computed_style::Display::Table(TableDisplay::FooterGroup),
                    "table-row" => computed_style::Display::Table(TableDisplay::Row),
                    "table-cell" => computed_style::Display::Table(TableDisplay::Cell),
                    "table-column-group" => computed_style::Display::Table(TableDisplay::ColumnGroup),
                    "table-column" => computed_style::Display::Table(TableDisplay::Column),
                    "table-caption" => computed_style::Display::Table(TableDisplay::Caption),
                    "list-item" => computed_style::Display::ListItem,
                    "contents" => computed_style::Display::Contents,
                    "none" => computed_style::Display::None,
                    _ => return None,
                })
            })?),
            BoxSizing => Value::BoxSizing(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "content-box" => computed_style::BoxSizing::ContentBox,
                    "border-box" => computed_style::BoxSizing::BorderBox,
                    _ => return None,
                })
            })?),
            Width | Height => Value::Size(SizeValue::parse(input, false)?),
            MinWidth | MinHeight => Value::AutoOr(auto_or(input, false)?),
            MarginTop | MarginRight | MarginBottom | MarginLeft | Top | Right | Bottom | Left => {
                Value::AutoOr(auto_or(input, true)?)
            }
            Position => Value::Position(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "static" => computed_style::Position::Static,
                    "relative" => computed_style::Position::Relative,
                    "absolute" => computed_style::Position::Absolute,
                    "fixed" => computed_style::Position::Fixed,
                    // Chrome's alias, which pages still write.
                    "sticky" | "-webkit-sticky" => computed_style::Position::Sticky,
                    _ => return None,
                })
            })?),
            // The flow-relative values (css-logical 1) aren't supported.
            Float => Value::Float(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "none" => computed_style::Float::None,
                    "left" => computed_style::Float::Left,
                    "right" => computed_style::Float::Right,
                    _ => return None,
                })
            })?),
            Content => Value::Content(parse_content(input)?),
            CounterReset => Value::CounterReset(parse_counter_reset(input)?),
            CounterIncrement => Value::CounterChanges(parse_counter_changes(input, 1)?),
            CounterSet => Value::CounterChanges(parse_counter_changes(input, 0)?),
            ListStyleType => Value::ListStyleType(parse_list_style_type(input)?),
            ListStylePosition => Value::ListStylePosition(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "outside" => computed_style::ListStylePosition::Outside,
                    "inside" => computed_style::ListStylePosition::Inside,
                    _ => return None,
                })
            })?),
            // Only `none`: machopo has no marker images.
            ListStyleImage => {
                input.expect_ident_matching("none")?;
                Value::ListStyleImage
            }
            Clear => Value::Clear(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "none" => computed_style::Clear::None,
                    "left" => computed_style::Clear::Left,
                    "right" => computed_style::Clear::Right,
                    "both" => computed_style::Clear::Both,
                    _ => return None,
                })
            })?),
            MaxWidth | MaxHeight => Value::NoneOr(
                if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
                    None
                } else {
                    Some(LengthPercentage::parse(input, false)?)
                },
            ),
            PaddingTop | PaddingRight | PaddingBottom | PaddingLeft => {
                Value::LengthPercentage(LengthPercentage::parse(input, false)?)
            }
            BorderTopWidth | BorderRightWidth | BorderBottomWidth | BorderLeftWidth
            | OutlineWidth => Value::LineWidth(LineWidth::parse(input)?),
            // `auto` is the platform's focus ring; `hidden` is a border's
            // only.
            OutlineStyle => Value::OutlineStyle(parse_keyword(input, |k| {
                use computed_style::{BorderStyle, OutlineStyle};
                match border_style(k) {
                    Some(BorderStyle::Hidden) => None,
                    Some(style) => Some(OutlineStyle::Line(style)),
                    None => k.eq_ignore_ascii_case("auto").then_some(OutlineStyle::Auto),
                }
            })?),
            OutlineOffset => Value::Length(Length::parse(input, true)?),
            BoxShadow => Value::BoxShadows(parse_shadows(input)?),
            TextShadow => Value::BoxShadows(crate::shadow::parse_text_shadows(input)?),
            Filter => Value::Filter(crate::filter::parse_filter(input)?),
            Isolation => Value::Isolation(parse_keyword(input, crate::filter::isolation)?),
            BorderTopStyle | BorderRightStyle | BorderBottomStyle | BorderLeftStyle => {
                Value::BorderStyle(parse_keyword(input, border_style)?)
            }
            BorderTopColor
            | BorderRightColor
            | BorderBottomColor
            | BorderLeftColor
            | OutlineColor
            | BackgroundColor
            | StopColor
            | TextDecorationColor
            | WebkitTextFillColor
            | WebkitTapHighlightColor
            | Color => Value::Color(parse_style_color(input)?),
            // `auto | <color>`.
            CaretColor => Value::CaretColor(
                if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                    None
                } else {
                    Some(parse_style_color(input)?)
                },
            ),
            // A horizontal radius, and a vertical one if it isn't the same.
            BorderTopLeftRadius
            | BorderTopRightRadius
            | BorderBottomRightRadius
            | BorderBottomLeftRadius => {
                let x = LengthPercentage::parse(input, false)?;
                let y = input.try_parse(|i| LengthPercentage::parse(i, false)).ok();
                Value::CornerRadius(x, y)
            }
            BackgroundImage => Value::BackgroundImages(parse_list(input, parse_image)?),
            BackgroundPositionX => Value::PositionsX(parse_list(input, |i| {
                SpecifiedPositionComponent::parse_axis(i, 0)
            })?),
            BackgroundPositionY => Value::PositionsY(parse_list(input, |i| {
                SpecifiedPositionComponent::parse_axis(i, 1)
            })?),
            BackgroundSize => {
                Value::BackgroundSizes(parse_list(input, SpecifiedBackgroundSize::parse)?)
            }
            BackgroundRepeat => Value::BackgroundRepeats(parse_list(input, parse_repeat)?),
            BackgroundAttachment => {
                Value::BackgroundAttachments(parse_list(input, parse_attachment)?)
            }
            BackgroundClip | BackgroundOrigin => {
                Value::BackgroundBoxes(parse_list(input, parse_box)?)
            }
            TextDecorationLine => Value::TextDecorationLine(parse_text_decoration_line(input)?),
            TextDecorationStyle => Value::TextDecorationStyle(parse_keyword(
                input,
                crate::text_decor::text_decoration_style,
            )?),
            TextDecorationThickness => {
                Value::TextDecorationThickness(crate::text_decor::SpecifiedThickness::parse(input)?)
            }
            TextUnderlineOffset => Value::AutoOr(auto_or(input, true)?),
            TextUnderlinePosition => {
                Value::TextUnderlinePosition(crate::text_decor::parse_underline_position(input)?)
            }
            // A percentage is the same number (`50%` is `0.5`); out of
            // range is clamped only when computed.
            Opacity | FillOpacity | StrokeOpacity | StopOpacity => {
                Value::Opacity(parse_alpha_value(input)?)
            }
            // `overlay` is a legacy alias of `auto` (css-overflow 3), which
            // Chrome resolves when parsing.
            OverflowX | OverflowY => Value::Overflow(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "visible" => OverflowValue::Visible,
                    "hidden" => OverflowValue::Hidden,
                    "clip" => OverflowValue::Clip,
                    "auto" | "overlay" => OverflowValue::Auto,
                    "scroll" => OverflowValue::Scroll,
                    _ => return None,
                })
            })?),
            Transform => Value::Transform(parse_transform(input)?),
            TransformOrigin => Value::TransformOrigin(parse_transform_origin(input)?),
            TransformBox => Value::TransformBox(parse_transform_box(input)?),
            ObjectFit => Value::ObjectFit(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "fill" => computed_style::ObjectFit::Fill,
                    "contain" => computed_style::ObjectFit::Contain,
                    "cover" => computed_style::ObjectFit::Cover,
                    "none" => computed_style::ObjectFit::None,
                    "scale-down" => computed_style::ObjectFit::ScaleDown,
                    _ => return None,
                })
            })?),
            ObjectPosition => Value::ObjectPosition(SpecifiedPosition::parse_position(input)?),
            // Blink's keywords: no `inline-axis` and `block-axis`.
            WebkitBoxOrient => Value::BoxOrient(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "horizontal" => computed_style::BoxOrient::Horizontal,
                    "vertical" => computed_style::BoxOrient::Vertical,
                    _ => return None,
                })
            })?),
            WebkitBoxDirection => Value::BoxDirection(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "normal" => computed_style::BoxDirection::Normal,
                    "reverse" => computed_style::BoxDirection::Reverse,
                    _ => return None,
                })
            })?),
            WebkitBoxPack => Value::BoxPack(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "start" => computed_style::BoxPack::Start,
                    "center" => computed_style::BoxPack::Center,
                    "end" => computed_style::BoxPack::End,
                    "justify" => computed_style::BoxPack::Justify,
                    _ => return None,
                })
            })?),
            WebkitBoxAlign => Value::BoxAlign(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "stretch" => computed_style::BoxAlign::Stretch,
                    "start" => computed_style::BoxAlign::Start,
                    "center" => computed_style::BoxAlign::Center,
                    "end" => computed_style::BoxAlign::End,
                    "baseline" => computed_style::BoxAlign::Baseline,
                    _ => return None,
                })
            })?),
            // Any number, negative too (Blink's).
            WebkitBoxFlex => Value::Number(parse_number(input)?),
            WebkitBoxOrdinalGroup => Value::Integer(parse_positive_integer(input)?),
            Contain => Value::Contain(parse_contain(input)?),
            Clip => Value::Clip(crate::masking::parse_clip(input)?),
            ClipPath => Value::ClipPath(crate::masking::parse_clip_path(input)?),
            // `none | <integer [1,∞]>`.
            WebkitLineClamp => {
                Value::LineClamp(match input.try_parse(|i| i.expect_ident_matching("none")) {
                    Ok(()) => None,
                    Err(_) => Some(parse_positive_integer(input)?),
                })
            }
            FlexDirection => Value::FlexDirection(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "row" => computed_style::FlexDirection::Row,
                    "row-reverse" => computed_style::FlexDirection::RowReverse,
                    "column" => computed_style::FlexDirection::Column,
                    "column-reverse" => computed_style::FlexDirection::ColumnReverse,
                    _ => return None,
                })
            })?),
            FlexWrap => Value::FlexWrap(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "nowrap" => computed_style::FlexWrap::Nowrap,
                    "wrap" => computed_style::FlexWrap::Wrap,
                    "wrap-reverse" => computed_style::FlexWrap::WrapReverse,
                    _ => return None,
                })
            })?),
            FlexGrow | FlexShrink => Value::Number(parse_non_negative_number(input)?),
            FlexBasis => Value::Size(SizeValue::parse(input, true)?),
            Order => Value::Integer(parse_integer(input)?),
            ZIndex => Value::AutoOrInteger(
                if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                    None
                } else {
                    Some(parse_integer(input)?)
                },
            ),
            JustifyContent | AlignContent => Value::ContentAlignment(parse_keyword(input, |k| {
                use computed_style::ContentAlignment::*;
                Some(match_ignore_ascii_case! { k,
                    "normal" => Normal,
                    "start" => Start,
                    "end" => End,
                    "flex-start" => FlexStart,
                    "flex-end" => FlexEnd,
                    "center" => Center,
                    "stretch" => Stretch,
                    "space-between" => SpaceBetween,
                    "space-around" => SpaceAround,
                    "space-evenly" => SpaceEvenly,
                    _ => return None,
                })
            })?),
            AlignItems | JustifyItems => Value::ItemAlignment(parse_item_alignment(input)?),
            AlignSelf | JustifySelf => Value::AlignSelf(
                if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                    None
                } else {
                    Some(parse_item_alignment(input)?)
                },
            ),
            RowGap | ColumnGap => Value::Gap(
                if input
                    .try_parse(|i| i.expect_ident_matching("normal"))
                    .is_ok()
                {
                    None
                } else {
                    Some(LengthPercentage::parse(input, false)?)
                },
            ),
            GridTemplateColumns | GridTemplateRows => Value::TrackList(parse_track_list(input)?),
            GridTemplateAreas => Value::Areas(parse_template_areas(input)?),
            GridAutoColumns | GridAutoRows => Value::TrackSizes(parse_track_sizes(input)?),
            GridAutoFlow => Value::GridAutoFlow(parse_auto_flow(input)?),
            GridRowStart | GridRowEnd | GridColumnStart | GridColumnEnd => {
                Value::GridLine(parse_grid_line(input)?)
            }
            FontFamily => Value::FontFamily(parse_font_family(input)?),
            FontSize => Value::FontSize(FontSizeValue::parse(input)?),
            FontWeight => Value::FontWeight(FontWeightValue::parse(input)?),
            FontStyle => Value::FontStyle(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "normal" => computed_style::FontStyle::Normal,
                    "italic" => computed_style::FontStyle::Italic,
                    "oblique" => computed_style::FontStyle::Oblique,
                    _ => return None,
                })
            })?),
            FontStretch => crate::font_features::parse_stretch(input)?,
            FontVariantLigatures => {
                Value::FontVariantLigatures(crate::font_features::parse_ligatures(input)?)
            }
            FontVariantNumeric => {
                Value::FontVariantNumeric(crate::font_features::parse_numeric(input)?)
            }
            FontVariantEastAsian => {
                Value::FontVariantEastAsian(crate::font_features::parse_east_asian(input)?)
            }
            FontVariantCaps => Value::FontVariantCaps(crate::font_features::parse_caps(input)?),
            FontVariantAlternates => {
                Value::FontVariantAlternates(crate::font_features::parse_alternates(input)?)
            }
            FontVariantPosition => {
                Value::FontVariantPosition(crate::font_features::parse_position(input)?)
            }
            FontVariantEmoji => Value::FontVariantEmoji(crate::font_features::parse_emoji(input)?),
            FontSizeAdjust => {
                Value::FontSizeAdjust(crate::font_features::parse_size_adjust(input)?)
            }
            FontLanguageOverride => {
                Value::FontLanguageOverride(crate::font_features::parse_language_override(input)?)
            }
            FontKerning => Value::FontKerning(crate::font_features::parse_kerning(input)?),
            FontOpticalSizing => {
                Value::FontOpticalSizing(crate::font_features::parse_optical_sizing(input)?)
            }
            FontFeatureSettings => {
                Value::FontFeatureSettings(crate::font_features::parse_feature_settings(input)?)
            }
            FontVariationSettings => {
                Value::FontVariationSettings(crate::font_features::parse_variation_settings(input)?)
            }
            FontSynthesisWeight | FontSynthesisStyle | FontSynthesisSmallCaps => {
                Value::FontSynthesis(crate::font_features::parse_synthesis_longhand(input)?)
            }
            LineHeight => Value::LineHeight(LineHeightValue::parse(input)?),
            TextAlign => Value::TextAlign(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "start" => computed_style::TextAlign::Start,
                    "end" => computed_style::TextAlign::End,
                    "left" => computed_style::TextAlign::Left,
                    "right" => computed_style::TextAlign::Right,
                    "center" => computed_style::TextAlign::Center,
                    _ => return None,
                })
            })?),
            VerticalAlign => {
                let keyword = input.try_parse(|i| {
                    parse_keyword(i, |k| {
                        Some(match_ignore_ascii_case! { k,
                            "baseline" => computed_style::VerticalAlign::Baseline,
                            "sub" => computed_style::VerticalAlign::Sub,
                            "super" => computed_style::VerticalAlign::Super,
                            "text-top" => computed_style::VerticalAlign::TextTop,
                            "text-bottom" => computed_style::VerticalAlign::TextBottom,
                            "middle" => computed_style::VerticalAlign::Middle,
                            "top" => computed_style::VerticalAlign::Top,
                            "bottom" => computed_style::VerticalAlign::Bottom,
                            _ => return None,
                        })
                    })
                });
                match keyword {
                    Ok(keyword) => Value::VerticalAlign(keyword),
                    Err(_) => Value::LengthPercentage(LengthPercentage::parse(input, true)?),
                }
            }
            WhiteSpaceCollapse => Value::WhiteSpaceCollapse(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "collapse" => computed_style::WhiteSpaceCollapse::Collapse,
                    "preserve" => computed_style::WhiteSpaceCollapse::Preserve,
                    "preserve-breaks" => computed_style::WhiteSpaceCollapse::PreserveBreaks,
                    "break-spaces" => computed_style::WhiteSpaceCollapse::BreakSpaces,
                    _ => return None,
                })
            })?),
            TextWrapMode => Value::TextWrapMode(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "wrap" => computed_style::TextWrapMode::Wrap,
                    "nowrap" => computed_style::TextWrapMode::Nowrap,
                    _ => return None,
                })
            })?),
            TextOverflow => Value::TextOverflow(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "clip" => computed_style::TextOverflow::Clip,
                    "ellipsis" => computed_style::TextOverflow::Ellipsis,
                    _ => return None,
                })
            })?),
            // Chrome's (152): no other `-webkit-appearance` keyword parses.
            Appearance => Value::Appearance(parse_keyword(input, |k| {
                use computed_style::Appearance::*;
                Some(match_ignore_ascii_case! { k,
                    "none" => None,
                    "auto" => Auto,
                    "checkbox" => Checkbox,
                    "radio" => Radio,
                    "button" => Button,
                    "listbox" => Listbox,
                    "menulist" => Menulist,
                    "menulist-button" => MenulistButton,
                    "meter" => Meter,
                    "progress-bar" => ProgressBar,
                    "searchfield" => Searchfield,
                    "textarea" => Textarea,
                    "textfield" => Textfield,
                    "base-select" => BaseSelect,
                    _ => return Option::None,
                })
            })?),
            TextTransform => Value::TextTransform(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "none" => computed_style::TextTransform::None,
                    "capitalize" => computed_style::TextTransform::Capitalize,
                    "uppercase" => computed_style::TextTransform::Uppercase,
                    "lowercase" => computed_style::TextTransform::Lowercase,
                    _ => return None,
                })
            })?),
            // `<number [0,∞]> | <length [0,∞]>`.
            TabSize => Value::TabSize(match input.try_parse(|i| parse_number_at_least(i, 0.0)) {
                Ok(spaces) => SpecifiedTabSize::Spaces(spaces),
                Err(_) => SpecifiedTabSize::Length(Length::parse(input, false)?),
            }),
            LetterSpacing | WordSpacing => Value::Spacing(
                match input.try_parse(|i| i.expect_ident_matching("normal")) {
                    Ok(()) => None,
                    Err(_) => Some(Length::parse(input, true)?),
                },
            ),
            TextIndent => Value::LengthPercentage(LengthPercentage::parse(input, true)?),
            WordBreak => Value::WordBreak(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "normal" => computed_style::WordBreak::Normal,
                    "break-all" => computed_style::WordBreak::BreakAll,
                    "keep-all" => computed_style::WordBreak::KeepAll,
                    "break-word" => computed_style::WordBreak::BreakWord,
                    _ => return None,
                })
            })?),
            OverflowWrap => Value::OverflowWrap(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "normal" => computed_style::OverflowWrap::Normal,
                    "break-word" => computed_style::OverflowWrap::BreakWord,
                    "anywhere" => computed_style::OverflowWrap::Anywhere,
                    _ => return None,
                })
            })?),
            Fill | Stroke => Value::Paint(SpecifiedPaint::parse(input)?),
            TextAnchor => Value::TextAnchor(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "start" => computed_style::TextAnchor::Start,
                    "middle" => computed_style::TextAnchor::Middle,
                    "end" => computed_style::TextAnchor::End,
                    _ => return None,
                })
            })?),
            FillRule | ClipRule => Value::FillRule(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "nonzero" => gfx::FillRule::NonZero,
                    "evenodd" => gfx::FillRule::EvenOdd,
                    _ => return None,
                })
            })?),
            // The SVG lengths take plain numbers, in user units.
            StrokeWidth | R => Value::LengthPercentage(LengthPercentage::parse_svg(input, false)?),
            StrokeDashoffset | Cx | Cy | X | Y => {
                Value::LengthPercentage(LengthPercentage::parse_svg(input, true)?)
            }
            Rx | Ry => Value::AutoOr(
                if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                    None
                } else {
                    Some(LengthPercentage::parse_svg(input, false)?)
                },
            ),
            StrokeLinecap => Value::StrokeLinecap(parse_keyword(input, |k| {
                use computed_style::StrokeLinecap::*;
                Some(match_ignore_ascii_case! { k,
                    "butt" => Butt,
                    "round" => Round,
                    "square" => Square,
                    _ => return None,
                })
            })?),
            StrokeLinejoin => Value::StrokeLinejoin(parse_keyword(input, |k| {
                use computed_style::StrokeLinejoin::*;
                Some(match_ignore_ascii_case! { k,
                    "miter" => Miter,
                    "round" => Round,
                    "bevel" => Bevel,
                    _ => return None,
                })
            })?),
            StrokeMiterlimit => Value::Number(parse_non_negative_number(input)?),
            StrokeDasharray => Value::Dasharray(parse_dasharray(input)?),
            D => Value::Path(parse_d(input)?),
            PointerEvents => Value::PointerEvents(parse_keyword(input, |k| {
                use computed_style::PointerEvents::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "none" => None,
                    "visiblepainted" => VisiblePainted,
                    "visiblefill" => VisibleFill,
                    "visiblestroke" => VisibleStroke,
                    "visible" => Visible,
                    "painted" => Painted,
                    "fill" => Fill,
                    "stroke" => Stroke,
                    "all" => All,
                    "bounding-box" => BoundingBox,
                    _ => return Option::None,
                })
            })?),
            BorderCollapse => Value::BorderCollapse(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "separate" => computed_style::BorderCollapse::Separate,
                    "collapse" => computed_style::BorderCollapse::Collapse,
                    _ => return None,
                })
            })?),
            WebkitBorderHorizontalSpacing | WebkitBorderVerticalSpacing => {
                Value::Length(Length::parse(input, false)?)
            }
            TableLayout => Value::TableLayout(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "auto" => computed_style::TableLayout::Auto,
                    "fixed" => computed_style::TableLayout::Fixed,
                    _ => return None,
                })
            })?),
            CaptionSide => Value::CaptionSide(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "top" => computed_style::CaptionSide::Top,
                    "bottom" => computed_style::CaptionSide::Bottom,
                    _ => return None,
                })
            })?),
            EmptyCells => Value::EmptyCells(parse_keyword(input, |k| {
                Some(match_ignore_ascii_case! { k,
                    "show" => computed_style::EmptyCells::Show,
                    "hide" => computed_style::EmptyCells::Hide,
                    _ => return None,
                })
            })?),
            // Chrome's values: `contain` isn't one.
            UserSelect => Value::UserSelect(parse_keyword(input, |k| {
                use computed_style::UserSelect::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "text" => Text,
                    "none" => None,
                    "all" => All,
                    _ => return Option::None,
                })
            })?),
            WebkitUserModify => Value::UserModify(parse_keyword(input, |k| {
                use computed_style::UserModify::*;
                Some(match_ignore_ascii_case! { k,
                    "read-only" => ReadOnly,
                    "read-write" => ReadWrite,
                    "read-write-plaintext-only" => ReadWritePlaintextOnly,
                    _ => return None,
                })
            })?),
            Hyphens => Value::Hyphens(parse_keyword(input, |k| {
                use computed_style::Hyphens::*;
                Some(match_ignore_ascii_case! { k,
                    "none" => None,
                    "manual" => Manual,
                    "auto" => Auto,
                    _ => return Option::None,
                })
            })?),
            ForcedColorAdjust => Value::ForcedColorAdjust(parse_keyword(input, |k| {
                use computed_style::ForcedColorAdjust::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "none" => None,
                    "preserve-parent-color" => PreserveParentColor,
                    _ => return Option::None,
                })
            })?),
            TextSizeAdjust => Value::TextSizeAdjust(parse_text_size_adjust(input)?),
            TextRendering => Value::TextRendering(parse_keyword(input, |k| {
                use computed_style::TextRendering::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "optimizespeed" => OptimizeSpeed,
                    "optimizelegibility" => OptimizeLegibility,
                    "geometricprecision" => GeometricPrecision,
                    _ => return None,
                })
            })?),
            // Chrome's values: `grayscale` is Firefox's `-moz-osx-font-
            // smoothing`.
            WebkitFontSmoothing => Value::FontSmoothing(parse_keyword(input, |k| {
                use computed_style::FontSmoothing::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "none" => None,
                    "antialiased" => Antialiased,
                    "subpixel-antialiased" => SubpixelAntialiased,
                    _ => return Option::None,
                })
            })?),
            Resize => Value::Resize(parse_keyword(input, |k| {
                use computed_style::Resize::*;
                Some(match_ignore_ascii_case! { k,
                    "none" => None,
                    "both" => Both,
                    "horizontal" => Horizontal,
                    "vertical" => Vertical,
                    "block" => Block,
                    "inline" => Inline,
                    "auto" => Auto,
                    _ => return Option::None,
                })
            })?),
            BorderImageSource => Value::BorderImageSource(crate::background::parse_image(input)?),
            BorderImageSlice => crate::border_image::parse_slice(input)?,
            BorderImageWidth => crate::border_image::parse_width(input)?,
            BorderImageOutset => crate::border_image::parse_outset(input)?,
            BorderImageRepeat => crate::border_image::parse_repeat(input)?,
            ScrollBehavior => Value::ScrollBehavior(parse_keyword(input, |k| {
                use computed_style::ScrollBehavior::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "smooth" => Smooth,
                    _ => return None,
                })
            })?),
            OverflowAnchor => Value::OverflowAnchor(parse_keyword(input, |k| {
                use computed_style::OverflowAnchor::*;
                Some(match_ignore_ascii_case! { k,
                    "auto" => Auto,
                    "none" => None,
                    _ => return Option::None,
                })
            })?),
            Cursor => Value::Cursor(crate::cursor::parse(input)?),
            TransitionProperty => Value::TransitionProperties(parse_transition_property(input)?),
            TransitionDuration => Value::Times(parse_list(input, |i| parse_time(i, true))?),
            TransitionTimingFunction => Value::Easings(parse_list(input, parse_easing)?),
            TransitionDelay => Value::Times(parse_list(input, |i| parse_time(i, false))?),
            TransitionBehavior => {
                Value::TransitionBehaviors(parse_list(input, parse_transition_behavior)?)
            }
            AnimationDuration => {
                Value::AnimationDurations(parse_list(input, parse_animation_duration)?)
            }
            AnimationTimingFunction => Value::Easings(parse_list(input, parse_easing)?),
            AnimationDelay => Value::Times(parse_list(input, |i| parse_time(i, false))?),
            AnimationIterationCount => {
                Value::IterationCounts(parse_list(input, parse_iteration_count)?)
            }
            AnimationDirection => {
                Value::PlaybackDirections(parse_list(input, parse_playback_direction)?)
            }
            AnimationFillMode => Value::FillModes(parse_list(input, parse_fill_mode)?),
            AnimationPlayState => Value::PlayStates(parse_list(input, parse_play_state)?),
            AnimationName => Value::AnimationNames(parse_list(input, parse_animation_name)?),
            AnimationTimeline => {
                Value::AnimationTimelines(parse_list(input, parse_animation_timeline)?)
            }
            AnimationRangeStart | AnimationRangeEnd => {
                Value::AnimationRangeBoundaries(parse_list(input, parse_animation_range_boundary)?)
            }
            Visibility => Value::Visibility(parse_keyword(input, |k| {
                use computed_style::Visibility::*;
                Some(match_ignore_ascii_case! { k,
                    "visible" => Visible,
                    "hidden" => Hidden,
                    "collapse" => Collapse,
                    _ => return None,
                })
            })?),
            WebkitTextSecurity => Value::TextSecurity(parse_keyword(input, |k| {
                use computed_style::TextSecurity::*;
                Some(match_ignore_ascii_case! { k,
                    "none" => None,
                    "disc" => Disc,
                    "circle" => Circle,
                    "square" => Square,
                    _ => return Option::None,
                })
            })?),
        })
    }
}

/// `<alpha-value>` as written: a number, or a percentage as the same
/// number (`50%` is `0.5`); out of range is clamped only when computed.
fn parse_alpha_value<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    if let Ok(value) = input.try_parse(crate::calc::parse_number) {
        return Ok(value);
    }
    let location = input.current_source_location();
    match *input.next()? {
        cssparser::Token::Number { value, .. } => Ok(value),
        cssparser::Token::Percentage { unit_value, .. } => Ok(unit_value),
        _ => Err(location.new_custom_error(())),
    }
}

/// `<line-style>`.
fn border_style(keyword: &str) -> Option<computed_style::BorderStyle> {
    use computed_style::BorderStyle::*;
    Some(match_ignore_ascii_case! { keyword,
        "none" => None,
        "hidden" => Hidden,
        "solid" => Solid,
        "dotted" => Dotted,
        "dashed" => Dashed,
        "double" => Double,
        "groove" => Groove,
        "ridge" => Ridge,
        "inset" => Inset,
        "outset" => Outset,
        _ => return Option::None,
    })
}

fn parse_keyword<'i, T>(
    input: &mut Parser<'i, '_>,
    f: impl FnOnce(&str) -> Option<T>,
) -> ParseResult<'i, T> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    f(ident).ok_or_else(|| location.new_custom_error(()))
}

/// `text-size-adjust`'s declared value: `none` reads back as written,
/// though it computes to 100%.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecifiedTextSizeAdjust {
    Auto,
    None,
    /// Non-negative; a math function's, as computed.
    Percent(f32),
}

fn parse_text_size_adjust<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, SpecifiedTextSizeAdjust> {
    if let Ok(keyword) = input.try_parse(|input| {
        parse_keyword(input, |k| {
            Some(match_ignore_ascii_case! { k,
                "auto" => SpecifiedTextSizeAdjust::Auto,
                "none" => SpecifiedTextSizeAdjust::None,
                _ => return None,
            })
        })
    }) {
        return Ok(keyword);
    }
    let location = input.current_source_location();
    let percent = match input.try_parse(crate::calc::parse_percentage) {
        Ok(percent) => percent,
        Err(_) => input.expect_percentage()? * 100.0,
    };
    if percent < 0.0 {
        return Err(location.new_custom_error(()));
    }
    Ok(SpecifiedTextSizeAdjust::Percent(percent))
}

/// `content` (css-content 3) as machopo has it: `normal | none | [ <string> |
/// attr(<ident>) | counter() | counters() ]+`. The other items (`url()`,
/// the quotes) and the alternative text after `/` aren't supported: the
/// declaration is invalid, as an unsupported value is.
fn parse_content<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, computed_style::Content> {
    use computed_style::{Content, ContentItem};
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(Content::Normal);
    }
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Content::None);
    }
    let mut items = Vec::new();
    while !input.is_exhausted() {
        let location = input.current_source_location();
        let item = match input.next()?.clone() {
            cssparser::Token::QuotedString(string) => ContentItem::String(Arc::from(&*string)),
            cssparser::Token::Function(name) if name.eq_ignore_ascii_case("attr") => input
                .parse_nested_block(|input| {
                    let name = input.expect_ident()?.clone();
                    Ok(ContentItem::Attr {
                        name: Arc::from(&*name),
                        value: Arc::from(""),
                    })
                })?,
            // `counter( <counter-name>, <counter-style>? )` and
            // `counters( <counter-name>, <string>, <counter-style>? )`.
            cssparser::Token::Function(name) if name.eq_ignore_ascii_case("counter") => input
                .parse_nested_block(|input| {
                    let name = parse_counter_name(input)?;
                    let style = parse_counter_style_argument(input)?;
                    Ok(ContentItem::Counter { name, style })
                })?,
            cssparser::Token::Function(name) if name.eq_ignore_ascii_case("counters") => input
                .parse_nested_block(|input| {
                    let name = parse_counter_name(input)?;
                    input.expect_comma()?;
                    let separator = Arc::from(&*input.expect_string()?.clone());
                    let style = parse_counter_style_argument(input)?;
                    Ok(ContentItem::Counters {
                        name,
                        separator,
                        style,
                    })
                })?,
            token => return Err(location.new_unexpected_token_error(token)),
        };
        items.push(item);
    }
    if items.is_empty() {
        return Err(input.new_custom_error(()));
    }
    Ok(Content::Items(items.into()))
}

/// A `<counter-name>`: a `<custom-ident>` other than `none`, kept as
/// written (counter names are case-sensitive).
fn parse_counter_name<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Arc<str>> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?.clone();
    match_ignore_ascii_case! { &ident,
        "none" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" => {
            Err(location.new_custom_error(()))
        },
        _ => Ok(Arc::from(&*ident)),
    }
}

/// `counter()`'s and `counters()`'s last argument, after a comma:
/// `decimal` if left out. A `<counter-style-name>` isn't `none`.
fn parse_counter_style_argument<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, computed_style::ListStyleType> {
    if input.is_exhausted() {
        return Ok(computed_style::ListStyleType::Decimal);
    }
    input.expect_comma()?;
    let location = input.current_source_location();
    match parse_counter_style(input)? {
        computed_style::ListStyleType::None => Err(location.new_custom_error(())),
        style => Ok(style),
    }
}

/// A `<counter-style>` by name (css-counter-styles 3), or `none`: one of
/// the predefined styles machopo has, or another name, kept as written
/// and rendered as `decimal` (the fallback of a style that isn't defined:
/// machopo has no `@counter-style`). `symbols()` isn't supported.
fn parse_counter_style<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, computed_style::ListStyleType> {
    use computed_style::ListStyleType as T;
    let location = input.current_source_location();
    let ident = input.expect_ident()?.clone();
    Ok(match_ignore_ascii_case! { &ident,
        "disc" => T::Disc,
        "circle" => T::Circle,
        "square" => T::Square,
        "disclosure-open" => T::DisclosureOpen,
        "disclosure-closed" => T::DisclosureClosed,
        "decimal" => T::Decimal,
        "decimal-leading-zero" => T::DecimalLeadingZero,
        "lower-roman" => T::LowerRoman,
        "upper-roman" => T::UpperRoman,
        "lower-greek" => T::LowerGreek,
        "lower-alpha" => T::LowerAlpha,
        "lower-latin" => T::LowerLatin,
        "upper-alpha" => T::UpperAlpha,
        "upper-latin" => T::UpperLatin,
        "none" => T::None,
        "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" => {
            return Err(location.new_custom_error(()));
        },
        _ => T::Named(Arc::from(&*ident)),
    })
}

/// `list-style-type`: `<counter-style> | <string> | none`.
fn parse_list_style_type<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, computed_style::ListStyleType> {
    if let Ok(string) = input.try_parse(|i| i.expect_string().cloned()) {
        return Ok(computed_style::ListStyleType::String(Arc::from(&*string)));
    }
    parse_counter_style(input)
}

/// `counter-reset`: `none | [ <counter-name> <integer>? |
/// reversed( <counter-name> ) <integer>? ]+`, a counter without a value
/// reset to 0, a `reversed()` one left to layout.
fn parse_counter_reset<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[computed_style::CounterReset]>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::new([]));
    }
    let mut counters = Vec::new();
    while !input.is_exhausted() {
        let reversed = input
            .try_parse(|i| {
                i.expect_function_matching("reversed")?;
                i.parse_nested_block(parse_counter_name)
            })
            .ok();
        let (name, is_reversed) = match reversed {
            Some(name) => (name, true),
            None => (parse_counter_name(input)?, false),
        };
        let value = input.try_parse(|i| i.expect_integer()).ok();
        counters.push(computed_style::CounterReset {
            name,
            value: value.or((!is_reversed).then_some(0)),
            reversed: is_reversed,
        });
    }
    if counters.is_empty() {
        return Err(input.new_custom_error(()));
    }
    Ok(counters.into())
}

/// `counter-increment` and `counter-set`: `none | [ <counter-name>
/// <integer>? ]+`, a counter without a value changed by or to `default`.
fn parse_counter_changes<'i>(
    input: &mut Parser<'i, '_>,
    default: i32,
) -> ParseResult<'i, Arc<[computed_style::CounterChange]>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::new([]));
    }
    let mut counters = Vec::new();
    while !input.is_exhausted() {
        let name = parse_counter_name(input)?;
        let value = input.try_parse(|i| i.expect_integer()).unwrap_or(default);
        counters.push(computed_style::CounterChange { name, value });
    }
    if counters.is_empty() {
        return Err(input.new_custom_error(()));
    }
    Ok(counters.into())
}

/// `none | [ underline || overline || line-through || blink ]`.
fn parse_text_decoration_line<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, TextDecorationLine> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(TextDecorationLine::NONE);
    }
    let mut line = TextDecorationLine::NONE;
    let mut any = false;
    loop {
        let result: ParseResult<'i, ()> = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?.clone();
            let flag = match_ignore_ascii_case! { &ident,
                "underline" => &mut line.underline,
                "overline" => &mut line.overline,
                "line-through" => &mut line.line_through,
                "blink" => &mut line.blink,
                _ => return Err(location.new_custom_error(())),
            };
            if *flag {
                return Err(location.new_custom_error(()));
            }
            *flag = true;
            Ok(())
        });
        if result.is_err() {
            break;
        }
        any = true;
    }
    if !any {
        return Err(input.new_custom_error(()));
    }
    Ok(line)
}

/// `contain` as written (Blink's `Contain::ParseSingleValue`): `none`,
/// `strict`, `content`, or any of `size` (or `inline-size`), `layout`,
/// `style` and `paint`, each once, in any order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecifiedContain {
    Strict,
    Content,
    /// `none` with nothing.
    Each(computed_style::Contain),
}

impl SpecifiedContain {
    fn compute(self) -> computed_style::Contain {
        match self {
            Self::Strict => computed_style::Contain::STRICT,
            Self::Content => computed_style::Contain::CONTENT,
            Self::Each(contain) => contain,
        }
    }
}

fn parse_contain<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedContain> {
    use computed_style::Contain;
    let keyword = |input: &mut Parser<'i, '_>| -> ParseResult<'i, String> {
        Ok(input.expect_ident()?.to_ascii_lowercase())
    };
    let first = input.try_parse(keyword)?;
    match first.as_str() {
        "none" => return Ok(SpecifiedContain::Each(Contain::NONE)),
        "strict" => return Ok(SpecifiedContain::Strict),
        "content" => return Ok(SpecifiedContain::Content),
        _ => {}
    }
    let mut contain = Contain::NONE;
    let mut next = Some(first);
    while let Some(word) = next.take() {
        let (kind, sizes) = match word.as_str() {
            "size" => (Contain::SIZE, true),
            "inline-size" => (Contain::INLINE_SIZE, true),
            "layout" => (Contain::LAYOUT, false),
            "style" => (Contain::STYLE, false),
            "paint" => (Contain::PAINT, false),
            _ => return Err(input.new_custom_error(())),
        };
        // Each once, and only one of `size` and `inline-size`.
        let taken = if sizes {
            contain.contains(Contain::INLINE_SIZE)
        } else {
            contain.contains(kind)
        };
        if taken {
            return Err(input.new_custom_error(()));
        }
        contain = contain.union(kind);
        next = input.try_parse(keyword).ok();
    }
    Ok(SpecifiedContain::Each(contain))
}

/// The keywords of `contain`: `none`, or what it has in the order `size`
/// (or `inline-size`), `layout`, `style`, `paint`; a computed value
/// (`computed`) that is exactly `strict` or `content` is that keyword
/// (Blink's `Contain::CSSValueFromComputedStyleInternal`), a specified one
/// keeps its keywords.
pub(crate) fn contain_to_css(contain: computed_style::Contain, computed: bool, dest: &mut String) {
    use computed_style::Contain;
    if contain == Contain::NONE {
        return dest.push_str("none");
    }
    if computed && contain == Contain::STRICT {
        return dest.push_str("strict");
    }
    if computed && contain == Contain::CONTENT {
        return dest.push_str("content");
    }
    let mut words = Vec::new();
    if contain.contains(Contain::SIZE) {
        words.push("size");
    } else if contain.contains(Contain::INLINE_SIZE) {
        words.push("inline-size");
    }
    for (kind, word) in [
        (Contain::LAYOUT, "layout"),
        (Contain::STYLE, "style"),
        (Contain::PAINT, "paint"),
    ] {
        if contain.contains(kind) {
            words.push(word);
        }
    }
    dest.push_str(&words.join(" "));
}

/// `<integer [1,∞]>`: below 1 is invalid, a math function's result is
/// clamped to 1 (css-values-4).
fn parse_positive_integer<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, i32> {
    if let Ok(value) = input.try_parse(crate::calc::parse_number) {
        return Ok(clamp_floor::<i32>(value + 0.5).max(1));
    }
    let value = input.expect_integer()?;
    if value < 1 {
        return Err(input.new_custom_error(()));
    }
    Ok(value)
}

/// `<number [0,∞]>`.
fn parse_non_negative_number<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    parse_number_at_least(input, 0.0)
}

/// `align-items`' values (`align-self`'s but `auto`).
fn parse_item_alignment<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, computed_style::ItemAlignment> {
    use computed_style::ItemAlignment::*;
    // `first baseline` is `baseline`.
    if input
        .try_parse(|i| i.expect_ident_matching("first"))
        .is_ok()
    {
        input.expect_ident_matching("baseline")?;
        return Ok(Baseline);
    }
    parse_keyword(input, |k| {
        Some(match_ignore_ascii_case! { k,
            "normal" => Normal,
            "stretch" => Stretch,
            "start" => Start,
            "end" => End,
            "self-start" => SelfStart,
            "self-end" => SelfEnd,
            "flex-start" => FlexStart,
            "flex-end" => FlexEnd,
            "center" => Center,
            "baseline" => Baseline,
            _ => return None,
        })
    })
}

/// `width`, `height` and `flex-basis` as declared.
#[derive(Debug, Clone, PartialEq)]
pub enum SizeValue {
    Auto,
    LengthPercentage(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent,
    /// `flex-basis` only.
    Content,
}

impl SizeValue {
    /// `auto | <length-percentage [0,∞]> | min-content | max-content |
    /// fit-content`, and `content` if `content` (`flex-basis`).
    fn parse<'i>(input: &mut Parser<'i, '_>, content: bool) -> ParseResult<'i, Self> {
        if let Ok(keyword) = input.try_parse(|i| {
            parse_keyword(i, |k| {
                Some(match_ignore_ascii_case! { k,
                    "auto" => Self::Auto,
                    "min-content" => Self::MinContent,
                    "max-content" => Self::MaxContent,
                    "fit-content" => Self::FitContent,
                    "content" if content => Self::Content,
                    _ => return None,
                })
            })
        }) {
            return Ok(keyword);
        }
        LengthPercentage::parse(input, false).map(Self::LengthPercentage)
    }

    /// The computed size; not for `content`, which only `flex-basis` takes.
    fn compute(&self, cx: &ComputeContext) -> computed_style::Size {
        use computed_style::Size;
        match self {
            Self::Auto => Size::Auto,
            Self::LengthPercentage(v) => Size::LengthPercentage(v.compute(cx)),
            Self::MinContent => Size::MinContent,
            Self::MaxContent => Size::MaxContent,
            Self::FitContent => Size::FitContent,
            Self::Content => unreachable!("only flex-basis takes content"),
        }
    }
}

fn auto_or<'i>(
    input: &mut Parser<'i, '_>,
    negative: bool,
) -> ParseResult<'i, Option<LengthPercentage>> {
    if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
        return Ok(None);
    }
    LengthPercentage::parse(input, negative).map(Some)
}

/// `tab-size` as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedTabSize {
    Spaces(f32),
    Length(Length),
}

/// `overflow-x` / `-y` as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowValue {
    Visible,
    Hidden,
    Clip,
    Auto,
    Scroll,
}

impl OverflowValue {
    fn compute(self) -> computed_style::Overflow {
        use computed_style::Overflow;
        match self {
            Self::Visible => Overflow::Visible,
            Self::Hidden => Overflow::Hidden,
            Self::Clip => Overflow::Clip,
            Self::Auto => Overflow::Auto,
            Self::Scroll => Overflow::Scroll,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssWideKeyword {
    Initial,
    Inherit,
    Unset,
}

/// A declared value. Which variant goes with which property is decided by
/// `LonghandId::parse_value`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Display(computed_style::Display),
    BoxSizing(computed_style::BoxSizing),
    Position(computed_style::Position),
    Float(computed_style::Float),
    Clear(computed_style::Clear),
    Content(computed_style::Content),
    CounterReset(Arc<[computed_style::CounterReset]>),
    /// `counter-increment` and `counter-set`.
    CounterChanges(Arc<[computed_style::CounterChange]>),
    ListStyleType(computed_style::ListStyleType),
    ListStylePosition(computed_style::ListStylePosition),
    /// `list-style-image: none`, the only value machopo takes.
    ListStyleImage,
    /// `None` is `auto`.
    AutoOr(Option<LengthPercentage>),
    Size(SizeValue),
    /// `None` is `none`.
    NoneOr(Option<LengthPercentage>),
    LengthPercentage(LengthPercentage),
    LineWidth(LineWidth),
    /// `outline-offset`.
    Length(Length),
    BorderStyle(computed_style::BorderStyle),
    OutlineStyle(computed_style::OutlineStyle),
    /// `box-shadow`; empty for `none`.
    BoxShadows(Arc<[SpecifiedShadow]>),
    Color(SpecifiedColor),
    /// `caret-color`: `None` is `auto`.
    CaretColor(Option<SpecifiedColor>),
    /// A corner's horizontal radius, and its vertical one if written.
    CornerRadius(LengthPercentage, Option<LengthPercentage>),
    /// The background longhands: one item per layer.
    BackgroundImages(Arc<[SpecifiedImage]>),
    /// `background-position-x` / `-y`: each axis has its own keywords.
    PositionsX(Arc<[SpecifiedPositionComponent]>),
    PositionsY(Arc<[SpecifiedPositionComponent]>),
    BackgroundSizes(Arc<[SpecifiedBackgroundSize]>),
    BackgroundRepeats(
        Arc<
            [(
                computed_style::BackgroundRepeat,
                computed_style::BackgroundRepeat,
            )],
        >,
    ),
    BackgroundAttachments(Arc<[computed_style::BackgroundAttachment]>),
    BackgroundBoxes(Arc<[computed_style::BackgroundBox]>),
    TextDecorationLine(TextDecorationLine),
    TextDecorationStyle(computed_style::TextDecorationStyle),
    TextDecorationThickness(crate::text_decor::SpecifiedThickness),
    TextUnderlinePosition(computed_style::TextUnderlinePosition),
    /// `filter`; empty for `none`.
    Filter(Arc<[crate::filter::SpecifiedFilter]>),
    Isolation(computed_style::Isolation),
    /// As written, not yet clamped to 0 to 1.
    Opacity(f32),
    Overflow(OverflowValue),
    /// `row-gap` / `column-gap`; `None` is `normal`.
    Gap(Option<LengthPercentage>),
    Transform(Arc<[TransformFunction]>),
    TransformOrigin(SpecifiedTransformOrigin),
    TransformBox(computed_style::TransformBox),
    TabSize(SpecifiedTabSize),
    ObjectFit(computed_style::ObjectFit),
    ObjectPosition(SpecifiedPosition),
    BoxOrient(computed_style::BoxOrient),
    BoxDirection(computed_style::BoxDirection),
    BoxPack(computed_style::BoxPack),
    BoxAlign(computed_style::BoxAlign),
    /// `-webkit-line-clamp`: `None` is `none`.
    LineClamp(Option<i32>),
    Contain(SpecifiedContain),
    Clip(crate::masking::SpecifiedClip),
    ClipPath(crate::masking::SpecifiedClipPath),
    FlexDirection(computed_style::FlexDirection),
    FlexWrap(computed_style::FlexWrap),
    Number(f32),
    Integer(i32),
    /// `None` is `auto`.
    AutoOrInteger(Option<i32>),
    ContentAlignment(computed_style::ContentAlignment),
    ItemAlignment(computed_style::ItemAlignment),
    /// `None` is `auto`.
    AlignSelf(Option<computed_style::ItemAlignment>),
    TrackList(SpecifiedTrackList),
    /// `None` is `none`.
    Areas(Option<Arc<computed_style::GridTemplateAreas>>),
    TrackSizes(Arc<[computed_style::TrackSize<LengthPercentage>]>),
    GridAutoFlow(computed_style::GridAutoFlow),
    GridLine(computed_style::GridLine),
    FontFamily(Arc<[computed_style::FontFamily]>),
    FontSize(FontSizeValue),
    FontWeight(FontWeightValue),
    FontStyle(computed_style::FontStyle),
    FontStretch(crate::font_features::SpecifiedFontStretch),
    FontVariantLigatures(computed_style::FontVariantLigatures),
    FontVariantNumeric(computed_style::FontVariantNumeric),
    FontVariantEastAsian(computed_style::FontVariantEastAsian),
    FontVariantCaps(computed_style::FontVariantCaps),
    FontVariantAlternates(computed_style::FontVariantAlternates),
    FontVariantPosition(computed_style::FontVariantPosition),
    FontVariantEmoji(computed_style::FontVariantEmoji),
    FontSizeAdjust(computed_style::FontSizeAdjust),
    /// `None` is `normal`.
    FontLanguageOverride(Option<Arc<str>>),
    FontKerning(computed_style::FontKerning),
    /// `auto`.
    FontOpticalSizing(bool),
    /// As written: every one, in order.
    FontFeatureSettings(Arc<[computed_style::FontFeatureSetting]>),
    /// As written: every one, in order.
    FontVariationSettings(Arc<[computed_style::FontVariationSetting]>),
    /// A `font-synthesis-*` longhand: `auto`.
    FontSynthesis(bool),
    LineHeight(LineHeightValue),
    TextAlign(computed_style::TextAlign),
    /// A keyword; a length is `LengthPercentage`.
    VerticalAlign(computed_style::VerticalAlign),
    WhiteSpaceCollapse(computed_style::WhiteSpaceCollapse),
    TextWrapMode(computed_style::TextWrapMode),
    TextTransform(computed_style::TextTransform),
    TextOverflow(computed_style::TextOverflow),
    Appearance(computed_style::Appearance),
    /// One of the `font` longhands, which a system font set: pending till
    /// it computes.
    SystemFont(crate::font::SystemFont),
    /// `letter-spacing` and `word-spacing`; `None` is `normal`.
    Spacing(Option<Length>),
    WordBreak(computed_style::WordBreak),
    OverflowWrap(computed_style::OverflowWrap),
    Paint(SpecifiedPaint),
    FillRule(gfx::FillRule),
    TextAnchor(computed_style::TextAnchor),
    StrokeLinecap(computed_style::StrokeLinecap),
    StrokeLinejoin(computed_style::StrokeLinejoin),
    /// `None` is `none`.
    Dasharray(Option<Arc<[LengthPercentage]>>),
    /// The path data as written; `None` is `none`.
    Path(Option<Arc<[computed_style::PathSegment]>>),
    Visibility(computed_style::Visibility),
    TextSecurity(computed_style::TextSecurity),
    PointerEvents(computed_style::PointerEvents),
    BorderCollapse(computed_style::BorderCollapse),
    TableLayout(computed_style::TableLayout),
    CaptionSide(computed_style::CaptionSide),
    EmptyCells(computed_style::EmptyCells),
    UserSelect(computed_style::UserSelect),
    UserModify(computed_style::UserModify),
    Hyphens(computed_style::Hyphens),
    ForcedColorAdjust(computed_style::ForcedColorAdjust),
    TextSizeAdjust(SpecifiedTextSizeAdjust),
    TextRendering(computed_style::TextRendering),
    FontSmoothing(computed_style::FontSmoothing),
    Resize(computed_style::Resize),
    ScrollBehavior(computed_style::ScrollBehavior),
    OverflowAnchor(computed_style::OverflowAnchor),
    Cursor(crate::cursor::SpecifiedCursor),
    BorderImageSource(SpecifiedImage),
    BorderImageSlice(computed_style::BorderImageSlice),
    /// Top, right, bottom, left.
    BorderImageWidth([crate::border_image::SpecifiedBorderImageWidth; 4]),
    BorderImageOutset([crate::border_image::SpecifiedBorderImageOutset; 4]),
    /// Horizontal, vertical.
    BorderImageRepeat(
        computed_style::BorderImageRepeat,
        computed_style::BorderImageRepeat,
    ),
    /// `transition-property`.
    TransitionProperties(Arc<[computed_style::TransitionProperty]>),
    /// `transition-duration` and `transition-delay`.
    Times(Arc<[SpecifiedTime]>),
    Easings(Arc<[SpecifiedEasing]>),
    TransitionBehaviors(Arc<[computed_style::TransitionBehavior]>),
    /// `animation-duration`: `None` is `auto`.
    AnimationDurations(Arc<[Option<SpecifiedTime>]>),
    /// `animation-iteration-count`: infinity is `infinite`.
    IterationCounts(Arc<[f32]>),
    PlaybackDirections(Arc<[computed_style::PlaybackDirection]>),
    FillModes(Arc<[computed_style::FillMode]>),
    PlayStates(Arc<[computed_style::AnimationPlayState]>),
    AnimationNames(Arc<[computed_style::AnimationName]>),
    AnimationTimelines(Arc<[computed_style::AnimationTimeline]>),
    /// `animation-range-start` or `-end`.
    AnimationRangeBoundaries(Arc<[computed_style::AnimationRangeBoundary]>),
    CssWide(CssWideKeyword),
    /// A value with `var()` in it, parsed only once substituted for an
    /// element (the cascade does it); a shorthand's longhands share it.
    Unparsed(Arc<UnparsedValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub id: LonghandId,
    pub value: Value,
}

impl Declaration {
    /// Resolves its `url()`s against `base`, those not resolved yet; one
    /// with `var()` keeps `base` for what it is substituted to. `unparsed`:
    /// those done so far, which a shorthand's longhands share.
    pub(crate) fn resolve_urls(
        &mut self,
        base: &UrlContext,
        unparsed: &mut Vec<(Arc<UnparsedValue>, Arc<UnparsedValue>)>,
    ) {
        match &mut self.value {
            Value::BackgroundImages(images) if images.iter().any(unresolved_image) => {
                let mut resolved = images.to_vec();
                for url in resolved.iter_mut().flat_map(SpecifiedImage::urls_mut) {
                    url.resolve(base);
                }
                *images = resolved.into();
            }
            Value::BorderImageSource(image) if image.has_unresolved_urls() => {
                for url in image.urls_mut() {
                    url.resolve(base);
                }
            }
            Value::Cursor(cursor) if cursor.has_unresolved_urls() => cursor.resolve_urls(base),
            Value::Paint(SpecifiedPaint::Url(url, _)) if !url.is_local() => url.resolve(base),
            Value::ClipPath(crate::masking::SpecifiedClipPath::Url(url)) if !url.is_local() => {
                url.resolve(base)
            }
            Value::Unparsed(value) if value.url_context.is_none() => {
                let done = unparsed.iter().find(|(old, _)| Arc::ptr_eq(old, value));
                let new = match done {
                    Some((_, new)) => new.clone(),
                    None => {
                        let new = Arc::new(UnparsedValue {
                            url_context: Some(base.clone()),
                            ..(**value).clone()
                        });
                        unparsed.push((value.clone(), new.clone()));
                        new
                    }
                };
                *value = new;
            }
            _ => {}
        }
    }

    /// Whether it has a `url()` not resolved yet.
    pub(crate) fn has_unresolved_urls(&self) -> bool {
        match &self.value {
            Value::BackgroundImages(images) => images.iter().any(unresolved_image),
            Value::BorderImageSource(image) => unresolved_image(image),
            Value::Cursor(cursor) => cursor.has_unresolved_urls(),
            Value::Paint(SpecifiedPaint::Url(url, _))
            | Value::ClipPath(crate::masking::SpecifiedClipPath::Url(url)) => {
                !url.is_local() && !url.is_resolved()
            }
            Value::Unparsed(value) => value.url_context.is_none(),
            _ => false,
        }
    }

    /// Itself with its `url()`s as if not resolved yet.
    pub(crate) fn as_written(&self) -> Self {
        let value = match &self.value {
            Value::BackgroundImages(images) => {
                Value::BackgroundImages(images.iter().map(SpecifiedImage::as_written).collect())
            }
            Value::BorderImageSource(image) => Value::BorderImageSource(image.as_written()),
            Value::Cursor(cursor) => Value::Cursor(cursor.as_written()),
            Value::Paint(SpecifiedPaint::Url(url, fallback)) => {
                Value::Paint(SpecifiedPaint::Url(url.as_written(), fallback.clone()))
            }
            Value::ClipPath(crate::masking::SpecifiedClipPath::Url(url)) => {
                Value::ClipPath(crate::masking::SpecifiedClipPath::Url(url.as_written()))
            }
            Value::Unparsed(value) => Value::Unparsed(Arc::new(UnparsedValue {
                url_context: None,
                ..(**value).clone()
            })),
            value => value.clone(),
        };
        Self { id: self.id, value }
    }
}

/// An image with a `url()` not resolved yet (one that doesn't resolve
/// stays so).
fn unresolved_image(image: &SpecifiedImage) -> bool {
    image.has_unresolved_urls()
}

/// Why a declaration was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationError {
    /// Not a property we support (maybe not one at all).
    UnsupportedProperty,
    /// A supported property with a value we can't parse.
    InvalidValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shorthand {
    /// `margin`, `padding`, `border-width` / `-style` / `-color`: one to
    /// four values for these longhands.
    Sides([LonghandId; 4]),
    /// `border-top` etc.: `<line-width> || <line-style> || <color>` for
    /// one side (index into the side arrays).
    BorderSide(usize),
    /// `border`: the same, for every side.
    Border,
    /// `outline`: `<outline-color> || <outline-style> || <outline-width>`.
    Outline,
    /// `border-radius`: one to four horizontal radii, then optionally `/`
    /// and one to four vertical ones (the same as the horizontal ones if
    /// left out), in corner order, repeated as `margin`'s sides are.
    BorderRadius,
    /// `border-spacing`: `<length>{1,2}`, horizontal then vertical (the
    /// horizontal one again if left out), into Chrome's two longhands.
    BorderSpacing,
    /// `border-image`: `<'border-image-source'> || <'border-image-slice'> [
    /// / <'border-image-width'> | / <'border-image-width'>? / <'border-
    /// image-outset'> ]? || <'border-image-repeat'>`.
    BorderImage,
    /// `background`: layers of `<bg-image> || <bg-position> [ / <bg-size>
    /// ]? || <repeat-style> || <attachment> || <visual-box> ||
    /// <visual-box>`, the last also taking a `<color>`.
    Background,
    /// `background-position`: `<bg-position>#`, into its x and y.
    BackgroundPosition,
    /// `[ <font-style> || <font-weight> ]? <font-size> [ / <line-height> ]?
    /// <font-family>`.
    Font,
    /// `font-variant`: `normal | none | [ <font-variant-ligatures>'s ||
    /// <font-variant-caps>'s || ... ]`, its seven longhands'.
    FontVariant,
    /// `font-synthesis`: `none | [ weight || style || small-caps ]`.
    FontSynthesis,
    /// `text-decoration`, of which only the line is supported.
    TextDecoration,
    /// `overflow`: one value for both axes, or x then y.
    Overflow,
    /// `white-space`: a keyword (`normal`, `pre`, `nowrap`, `pre-wrap`,
    /// `pre-line`, `break-spaces`), or `<'white-space-collapse'> ||
    /// <'text-wrap-mode'>` (css-text 4, as Chrome has it).
    WhiteSpace,
    /// `text-wrap`, of which only the mode is supported (`balance` and the
    /// other styles aren't).
    TextWrap,
    /// `none | [ <flex-grow> <flex-shrink>? || <flex-basis> ]`.
    Flex,
    /// `<flex-direction> || <flex-wrap>`.
    FlexFlow,
    /// `<'list-style-position'> || <'list-style-image'> ||
    /// <'list-style-type'>`, a lone `none` either of the last two (both
    /// if neither is given otherwise).
    ListStyle,
    /// `gap` (and its legacy name `grid-gap`): the row gap, then the
    /// column gap (the same if omitted).
    Gap,
    /// `grid-row` / `grid-column`: the start, then the end after a `/`
    /// (if omitted: the start if it is only a name, `auto` otherwise).
    GridLines([LonghandId; 2]),
    /// `grid-area`: row start, column start, row end, column end, each
    /// after a `/` (if omitted: as for `grid-row`, from the row start for
    /// the column start and row end, from the column start for the column
    /// end).
    GridArea,
    /// `grid-template`: `none`, rows `/` columns, or each row's area
    /// string with its line names and size, then `/` and the columns.
    GridTemplate,
    /// `place-content` / `-items` / `-self`: the `align-*` value, then the
    /// `justify-*` one (the same if omitted).
    Place([LonghandId; 2]),
    /// `margin-block`, `border-inline-width` and their kin (`PAIRS`): the
    /// start, then the end (the same if omitted).
    Pair([LonghandId; 2]),
    /// `border-block-start` etc.: `border-top`'s value for one
    /// flow-relative side (index into the `FLOW_BORDER_*` arrays).
    FlowBorderSide(usize),
    /// `border-block` (0) and `border-inline` (1): the same, for both
    /// sides of the axis.
    BorderAxis(usize),
    /// `<single-transition>#`.
    Transition,
    /// `<single-animation>#`.
    Animation,
    /// `animation-range`: each animation's start and end.
    AnimationRange,
}

impl Shorthand {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        if let Some((_, ids)) = PAIRS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            return Some(Self::Pair(*ids));
        }
        Some(match_ignore_ascii_case! { name,
            "margin" => Self::Sides(MARGIN),
            "padding" => Self::Sides(PADDING),
            "inset" => Self::Sides(INSET),
            "border-width" => Self::Sides(BORDER_WIDTH),
            "border-style" => Self::Sides(BORDER_STYLE),
            "border-color" => Self::Sides(BORDER_COLOR),
            "border-top" => Self::BorderSide(0),
            "border-right" => Self::BorderSide(1),
            "border-bottom" => Self::BorderSide(2),
            "border-left" => Self::BorderSide(3),
            "border" => Self::Border,
            "border-radius" => Self::BorderRadius,
            "border-image" => Self::BorderImage,
            "border-spacing" => Self::BorderSpacing,
            "outline" => Self::Outline,
            "background" => Self::Background,
            "background-position" => Self::BackgroundPosition,
            "font" => Self::Font,
            "font-variant" => Self::FontVariant,
            "font-synthesis" => Self::FontSynthesis,
            "text-decoration" => Self::TextDecoration,
            "overflow" => Self::Overflow,
            "white-space" => Self::WhiteSpace,
            "text-wrap" => Self::TextWrap,
            "flex" => Self::Flex,
            "flex-flow" => Self::FlexFlow,
            "list-style" => Self::ListStyle,
            "gap" | "grid-gap" => Self::Gap,
            "grid-row" => Self::GridLines([GridRowStart, GridRowEnd]),
            "grid-column" => Self::GridLines([GridColumnStart, GridColumnEnd]),
            "grid-area" => Self::GridArea,
            "grid-template" => Self::GridTemplate,
            "place-content" => Self::Place([AlignContent, JustifyContent]),
            "place-items" => Self::Place([AlignItems, JustifyItems]),
            "place-self" => Self::Place([AlignSelf, JustifySelf]),
            "border-block-start" => Self::FlowBorderSide(0),
            "border-block-end" => Self::FlowBorderSide(1),
            "border-inline-start" => Self::FlowBorderSide(2),
            "border-inline-end" => Self::FlowBorderSide(3),
            "border-block" => Self::BorderAxis(0),
            "border-inline" => Self::BorderAxis(1),
            "transition" => Self::Transition,
            "animation" => Self::Animation,
            "animation-range" => Self::AnimationRange,
            _ => return None,
        })
    }

    /// The shorthand's name (for an alias, the standard one).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Sides(MARGIN) => "margin",
            Self::Sides(PADDING) => "padding",
            Self::Sides(BORDER_WIDTH) => "border-width",
            Self::Sides(BORDER_STYLE) => "border-style",
            Self::Sides(BORDER_COLOR) => "border-color",
            Self::Sides(INSET) => "inset",
            Self::Sides(_) => unreachable!("not a shorthand"),
            Self::BorderSide(i) => {
                ["border-top", "border-right", "border-bottom", "border-left"][i]
            }
            Self::Border => "border",
            Self::BorderRadius => "border-radius",
            Self::BorderImage => "border-image",
            Self::BorderSpacing => "border-spacing",
            Self::Outline => "outline",
            Self::Background => "background",
            Self::BackgroundPosition => "background-position",
            Self::Font => "font",
            Self::FontVariant => "font-variant",
            Self::FontSynthesis => "font-synthesis",
            Self::TextDecoration => "text-decoration",
            Self::Overflow => "overflow",
            Self::WhiteSpace => "white-space",
            Self::TextWrap => "text-wrap",
            Self::Flex => "flex",
            Self::FlexFlow => "flex-flow",
            Self::ListStyle => "list-style",
            Self::Gap => "gap",
            Self::GridLines([GridRowStart, _]) => "grid-row",
            Self::GridLines(_) => "grid-column",
            Self::GridArea => "grid-area",
            Self::GridTemplate => "grid-template",
            Self::Place([AlignContent, _]) => "place-content",
            Self::Place([AlignItems, _]) => "place-items",
            Self::Place(_) => "place-self",
            Self::Pair(ids) => {
                PAIRS
                    .iter()
                    .find(|(_, pair)| *pair == ids)
                    .expect("a shorthand")
                    .0
            }
            Self::FlowBorderSide(i) => [
                "border-block-start",
                "border-block-end",
                "border-inline-start",
                "border-inline-end",
            ][i],
            Self::BorderAxis(axis) => ["border-block", "border-inline"][axis],
            Self::Transition => "transition",
            Self::Animation => "animation",
            Self::AnimationRange => "animation-range",
        }
    }

    /// Every shorthand, each once.
    pub(crate) const ALL: [Self; 56] = [
        Self::Sides(MARGIN),
        Self::Sides(PADDING),
        Self::Sides(INSET),
        Self::Sides(BORDER_WIDTH),
        Self::Sides(BORDER_STYLE),
        Self::Sides(BORDER_COLOR),
        Self::BorderSide(0),
        Self::BorderSide(1),
        Self::BorderSide(2),
        Self::BorderSide(3),
        Self::Border,
        Self::BorderRadius,
        Self::BorderImage,
        Self::BorderSpacing,
        Self::Outline,
        Self::Background,
        Self::BackgroundPosition,
        Self::Font,
        Self::FontVariant,
        Self::FontSynthesis,
        Self::TextDecoration,
        Self::Overflow,
        Self::WhiteSpace,
        Self::TextWrap,
        Self::Flex,
        Self::FlexFlow,
        Self::ListStyle,
        Self::Gap,
        Self::GridLines([GridRowStart, GridRowEnd]),
        Self::GridLines([GridColumnStart, GridColumnEnd]),
        Self::GridArea,
        Self::GridTemplate,
        Self::Place([AlignContent, JustifyContent]),
        Self::Place([AlignItems, JustifyItems]),
        Self::Place([AlignSelf, JustifySelf]),
        Self::Pair(MARGIN_BLOCK),
        Self::Pair(MARGIN_INLINE),
        Self::Pair(PADDING_BLOCK),
        Self::Pair(PADDING_INLINE),
        Self::Pair(INSET_BLOCK),
        Self::Pair(INSET_INLINE),
        Self::Pair(BORDER_BLOCK_WIDTH),
        Self::Pair(BORDER_BLOCK_STYLE),
        Self::Pair(BORDER_BLOCK_COLOR),
        Self::Pair(BORDER_INLINE_WIDTH),
        Self::Pair(BORDER_INLINE_STYLE),
        Self::Pair(BORDER_INLINE_COLOR),
        Self::FlowBorderSide(0),
        Self::FlowBorderSide(1),
        Self::FlowBorderSide(2),
        Self::FlowBorderSide(3),
        Self::BorderAxis(0),
        Self::BorderAxis(1),
        Self::Transition,
        Self::Animation,
        Self::AnimationRange,
    ];

    /// Whether Chrome's shorthand also sets longhands machopo doesn't have
    /// (`text-wrap-style`):
    /// Chrome serializes the block with it only if those are there too,
    /// which happens only when it was set.
    pub(crate) fn is_partial(self) -> bool {
        matches!(self, Self::TextWrap)
    }

    pub(crate) fn longhands(self) -> Vec<LonghandId> {
        match self {
            Self::Sides(sides) => sides.to_vec(),
            Self::BorderSide(i) => vec![BORDER_WIDTH[i], BORDER_STYLE[i], BORDER_COLOR[i]],
            // It resets `border-image` too (css-backgrounds 3).
            Self::Border => [
                &BORDER_WIDTH[..],
                &BORDER_STYLE,
                &BORDER_COLOR,
                &BORDER_IMAGE,
            ]
            .concat(),
            Self::BorderRadius => BORDER_RADIUS.to_vec(),
            Self::BorderImage => BORDER_IMAGE.to_vec(),
            Self::BorderSpacing => vec![WebkitBorderHorizontalSpacing, WebkitBorderVerticalSpacing],
            // Blink's order, which its serialization goes by.
            Self::Outline => vec![OutlineColor, OutlineStyle, OutlineWidth],
            // Blink's order, which its serialization goes by.
            Self::Background => BACKGROUND.to_vec(),
            Self::BackgroundPosition => vec![BackgroundPositionX, BackgroundPositionY],
            Self::Font => FONT.to_vec(),
            Self::FontVariant => FONT_VARIANT.to_vec(),
            Self::FontSynthesis => vec![
                FontSynthesisWeight,
                FontSynthesisStyle,
                FontSynthesisSmallCaps,
            ],
            // Blink's order, which its serialization goes by.
            Self::TextDecoration => vec![
                TextDecorationLine,
                TextDecorationThickness,
                TextDecorationStyle,
                TextDecorationColor,
            ],
            Self::Overflow => vec![OverflowX, OverflowY],
            Self::WhiteSpace => vec![WhiteSpaceCollapse, TextWrapMode],
            Self::TextWrap => vec![TextWrapMode],
            Self::Flex => vec![FlexGrow, FlexShrink, FlexBasis],
            Self::FlexFlow => vec![FlexDirection, FlexWrap],
            Self::ListStyle => vec![ListStylePosition, ListStyleImage, ListStyleType],
            Self::Gap => vec![RowGap, ColumnGap],
            Self::GridLines(ids) | Self::Place(ids) | Self::Pair(ids) => ids.to_vec(),
            Self::FlowBorderSide(i) => {
                vec![
                    FLOW_BORDER_WIDTH[i],
                    FLOW_BORDER_STYLE[i],
                    FLOW_BORDER_COLOR[i],
                ]
            }
            // As `border`'s: the widths, then the styles, then the colors.
            Self::BorderAxis(axis) => {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "an axis is 0 or 1: its sides are 0 and 1, or 2 and 3"
                )]
                let (first, second) = (2 * axis, 2 * axis + 1);
                [FLOW_BORDER_WIDTH, FLOW_BORDER_STYLE, FLOW_BORDER_COLOR]
                    .iter()
                    .flat_map(|sides| [sides[first], sides[second]])
                    .collect()
            }
            Self::GridArea => vec![GridRowStart, GridColumnStart, GridRowEnd, GridColumnEnd],
            Self::GridTemplate => vec![GridTemplateRows, GridTemplateColumns, GridTemplateAreas],
            Self::Transition => TRANSITION.to_vec(),
            Self::Animation => [&ANIMATION[..], &ANIMATION_RESET].concat(),
            Self::AnimationRange => vec![AnimationRangeStart, AnimationRangeEnd],
        }
    }

    /// Values for `longhands()`, in the same order, each with whether the
    /// shorthand left it out (and so set it to its initial value).
    fn parse<'i>(self, input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<(Value, bool)>> {
        match self {
            Self::BorderSide(_) | Self::FlowBorderSide(_) => {
                parse_border_side(input).map(|v| v.to_vec())
            }
            Self::BorderAxis(_) => {
                let [width, style, color] = parse_border_side(input)?;
                Ok([vec![width; 2], vec![style; 2], vec![color; 2]].concat())
            }
            Self::Border => {
                let [width, style, color] = parse_border_side(input)?;
                let image = crate::border_image::initial_values().map(|value| (value, true));
                Ok([
                    vec![width; 4],
                    vec![style; 4],
                    vec![color; 4],
                    image.to_vec(),
                ]
                .concat())
            }
            Self::Background => parse_background(input),
            Self::Animation => parse_animation(input),
            Self::BorderRadius => parse_border_radius(input),
            Self::BorderImage => crate::border_image::parse_border_image(input),
            Self::Outline => parse_outline(input).map(|v| v.to_vec()),
            Self::TextDecoration => parse_text_decoration(input).map(|v| v.to_vec()),
            Self::BackgroundPosition => {
                let positions = parse_list(input, SpecifiedPosition::parse)?;
                Ok(vec![
                    (
                        Value::PositionsX(positions.iter().map(|p| p.x.clone()).collect()),
                        false,
                    ),
                    (
                        Value::PositionsY(positions.iter().map(|p| p.y.clone()).collect()),
                        false,
                    ),
                ])
            }
            _ => Ok(self
                .parse_values(input)?
                .into_iter()
                .map(|value| (value, false))
                .collect()),
        }
    }

    /// [`parse`](Self::parse) for the shorthands that leave nothing out.
    fn parse_values<'i>(self, input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
        match self {
            Self::BorderSide(_)
            | Self::FlowBorderSide(_)
            | Self::BorderAxis(_)
            | Self::Border
            | Self::BorderRadius
            | Self::BorderImage
            | Self::Outline
            | Self::TextDecoration
            | Self::Background
            | Self::Animation
            | Self::BackgroundPosition => {
                unreachable!("parsed by Shorthand::parse")
            }
            Self::Sides(sides) => parse_sides(input, sides[0]),
            Self::Font => parse_font(input),
            Self::FontVariant => crate::font_features::parse_font_variant(input),
            Self::FontSynthesis => Ok(crate::font_features::parse_synthesis(input)?
                .map(Value::FontSynthesis)
                .to_vec()),
            Self::BorderSpacing => {
                let horizontal = Length::parse(input, false)?;
                let vertical = input
                    .try_parse(|i| Length::parse(i, false))
                    .unwrap_or_else(|_| horizontal.clone());
                Ok(vec![Value::Length(horizontal), Value::Length(vertical)])
            }
            Self::Flex => parse_flex(input),
            Self::FlexFlow => {
                let (mut direction, mut wrap) = (None, None);
                loop {
                    if direction.is_none()
                        && let Ok(v) = input.try_parse(|i| FlexDirection.parse_value(i))
                    {
                        direction = Some(v);
                        continue;
                    }
                    if wrap.is_none()
                        && let Ok(v) = input.try_parse(|i| FlexWrap.parse_value(i))
                    {
                        wrap = Some(v);
                        continue;
                    }
                    break;
                }
                if direction.is_none() && wrap.is_none() {
                    return Err(input.new_custom_error(()));
                }
                Ok(vec![
                    direction.unwrap_or(Value::FlexDirection(computed_style::FlexDirection::Row)),
                    wrap.unwrap_or(Value::FlexWrap(computed_style::FlexWrap::Nowrap)),
                ])
            }
            Self::ListStyle => {
                let (mut position, mut image, mut kind) = (None, None, None);
                let mut nones = 0;
                loop {
                    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
                        // Only compared with 1 and 2: saturating changes
                        // nothing.
                        nones = u32::saturating_add(nones, 1);
                        continue;
                    }
                    if position.is_none()
                        && let Ok(v) = input.try_parse(|i| ListStylePosition.parse_value(i))
                    {
                        position = Some(v);
                        continue;
                    }
                    if image.is_none()
                        && let Ok(v) = input.try_parse(|i| ListStyleImage.parse_value(i))
                    {
                        image = Some(v);
                        continue;
                    }
                    if kind.is_none()
                        && let Ok(v) = input.try_parse(|i| ListStyleType.parse_value(i))
                    {
                        kind = Some(v);
                        continue;
                    }
                    break;
                }
                // Each `none` is the image's or the type's, whichever
                // nothing else gave.
                let none_type = Value::ListStyleType(computed_style::ListStyleType::None);
                match (nones, kind.is_some()) {
                    (0, _) => {}
                    (1, false) => kind = Some(none_type),
                    (1, true) | (2, false) => {
                        image = Some(Value::ListStyleImage);
                        kind = kind.or(Some(none_type));
                    }
                    _ => return Err(input.new_custom_error(())),
                }
                if position.is_none() && image.is_none() && kind.is_none() {
                    return Err(input.new_custom_error(()));
                }
                Ok(vec![
                    position.unwrap_or(Value::ListStylePosition(
                        computed_style::ListStylePosition::Outside,
                    )),
                    image.unwrap_or(Value::ListStyleImage),
                    kind.unwrap_or(Value::ListStyleType(computed_style::ListStyleType::Disc)),
                ])
            }
            Self::Gap => {
                let row = RowGap.parse_value(input)?;
                let column = input
                    .try_parse(|i| ColumnGap.parse_value(i))
                    .unwrap_or_else(|_| row.clone());
                Ok(vec![row, column])
            }
            Self::GridLines(_) | Self::GridArea => {
                let count = if self == Self::GridArea { 4 } else { 2 };
                let mut lines = vec![parse_grid_line_and_name(input)?];
                while lines.len() < count && input.try_parse(|i| i.expect_delim('/')).is_ok() {
                    lines.push(parse_grid_line_and_name(input)?);
                }
                // An omitted line is the name of the one it pairs with, if
                // that is only a name: the start's for the end, the row
                // start's for the column start (`grid-area`).
                let copy = |(_, name): &(computed_style::GridLine, Option<Arc<str>>)| match name {
                    Some(name) => (
                        computed_style::GridLine::Ident(name.clone()),
                        Some(name.clone()),
                    ),
                    None => (computed_style::GridLine::Auto, None),
                };
                // Which line an omitted one copies: for `grid-area`, row
                // start, column start, row end, column end.
                let pairs: &[usize] = if count == 4 { &[0, 0, 0, 1] } else { &[0, 0] };
                while lines.len() < count {
                    let line = copy(&lines[pairs[lines.len()]]);
                    lines.push(line);
                }
                Ok(lines
                    .into_iter()
                    .map(|(line, _)| Value::GridLine(line))
                    .collect())
            }
            Self::GridTemplate => parse_grid_template(input),
            Self::Transition => parse_transition(input),
            Self::AnimationRange => parse_animation_range(input),
            Self::Place([align, justify]) | Self::Pair([align, justify]) => {
                let first = align.parse_value(input)?;
                let second = input
                    .try_parse(|i| justify.parse_value(i))
                    .unwrap_or_else(|_| first.clone());
                Ok(vec![first, second])
            }
            Self::WhiteSpace => {
                // A keyword alone (`nowrap preserve` is the longhands).
                let keyword = input.try_parse(|i| -> ParseResult<'i, computed_style::WhiteSpace> {
                    let keyword = parse_keyword(i, |k| {
                        Some(match_ignore_ascii_case! { k,
                            "normal" => computed_style::WhiteSpace::Normal,
                            "pre" => computed_style::WhiteSpace::Pre,
                            "nowrap" => computed_style::WhiteSpace::Nowrap,
                            "pre-wrap" => computed_style::WhiteSpace::PreWrap,
                            "pre-line" => computed_style::WhiteSpace::PreLine,
                            "break-spaces" => computed_style::WhiteSpace::BreakSpaces,
                            _ => return None,
                        })
                    })?;
                    i.expect_exhausted()?;
                    Ok(keyword)
                });
                if let Ok(keyword) = keyword {
                    let (collapse, wrap) = keyword.longhands();
                    return Ok(vec![
                        Value::WhiteSpaceCollapse(collapse),
                        Value::TextWrapMode(wrap),
                    ]);
                }
                let (mut collapse, mut wrap) = (None, None);
                loop {
                    if collapse.is_none()
                        && let Ok(v) = input.try_parse(|i| WhiteSpaceCollapse.parse_value(i))
                    {
                        collapse = Some(v);
                        continue;
                    }
                    if wrap.is_none()
                        && let Ok(v) = input.try_parse(|i| TextWrapMode.parse_value(i))
                    {
                        wrap = Some(v);
                        continue;
                    }
                    break;
                }
                if collapse.is_none() && wrap.is_none() {
                    return Err(input.new_custom_error(()));
                }
                Ok(vec![
                    collapse.unwrap_or(Value::WhiteSpaceCollapse(
                        computed_style::WhiteSpaceCollapse::Collapse,
                    )),
                    wrap.unwrap_or(Value::TextWrapMode(computed_style::TextWrapMode::Wrap)),
                ])
            }
            Self::TextWrap => Ok(vec![TextWrapMode.parse_value(input)?]),
            Self::Overflow => {
                let x = OverflowX.parse_value(input)?;
                let y = input
                    .try_parse(|i| OverflowY.parse_value(i))
                    .unwrap_or_else(|_| x.clone());
                Ok(vec![x, y])
            }
        }
    }
}

/// The longhands property `name` sets: a shorthand's, or the longhand
/// itself; `None` for a name machopo has no property of.
pub fn longhands_of(name: &str) -> Option<Vec<LonghandId>> {
    match Shorthand::from_name(name) {
        Some(shorthand) => Some(shorthand.longhands()),
        None => LonghandId::from_name(name).map(|id| vec![id]),
    }
}

/// Parses the value of property `name` (up to `!important`, which the
/// caller handles) into longhand declarations, each with whether a
/// shorthand left it out, and the shorthand they come from.
#[allow(clippy::type_complexity)]
pub fn parse_declaration(
    name: &str,
    input: &mut Parser<'_, '_>,
) -> Result<(Option<Shorthand>, Vec<(Declaration, bool)>), DeclarationError> {
    let shorthand = Shorthand::from_name(name);
    let longhands = match (shorthand, LonghandId::from_name(name)) {
        (Some(shorthand), _) => shorthand.longhands(),
        (None, Some(id)) => vec![id],
        (None, None) => return Err(DeclarationError::UnsupportedProperty),
    };
    parse_longhands(shorthand, longhands, input).map(|declarations| (shorthand, declarations))
}

pub(crate) fn parse_longhands(
    shorthand: Option<Shorthand>,
    longhands: Vec<LonghandId>,
    input: &mut Parser<'_, '_>,
) -> Result<Vec<(Declaration, bool)>, DeclarationError> {
    let wide = input.try_parse(|i| {
        let location = i.current_source_location();
        let ident = i.expect_ident()?.clone();
        let keyword = match_ignore_ascii_case! { &ident,
            "initial" => CssWideKeyword::Initial,
            "inherit" => CssWideKeyword::Inherit,
            "unset" => CssWideKeyword::Unset,
            _ => return Err(location.new_custom_error(())),
        };
        i.expect_exhausted()?;
        Ok::<_, cssparser::ParseError<'_, ()>>(keyword)
    });
    if let Ok(keyword) = wide {
        return Ok(longhands
            .into_iter()
            .map(|id| {
                let value = Value::CssWide(keyword);
                (Declaration { id, value }, false)
            })
            .collect());
    }

    // A value with `var()` is kept as written, for each longhand.
    let start = input.position();
    let with_var = input.try_parse(|i| match check_declaration_value::<()>(i, true) {
        Ok(true) => Ok(i.slice_from(start)),
        _ => Err(i.new_custom_error::<_, ()>(())),
    });
    if let Ok(css) = with_var {
        let unparsed = UnparsedValue::new(trim_value(css), shorthand);
        return Ok(longhands
            .into_iter()
            .map(|id| {
                let value = Value::Unparsed(unparsed.clone());
                (Declaration { id, value }, false)
            })
            .collect());
    }

    let values = match shorthand {
        Some(shorthand) => shorthand.parse(input),
        None => longhands[0].parse_value(input).map(|v| vec![(v, false)]),
    };
    let values = values
        .and_then(|values| {
            input
                .expect_exhausted()
                .map(|()| values)
                .map_err(Into::into)
        })
        .map_err(|_| DeclarationError::InvalidValue)?;
    debug_assert_eq!(values.len(), longhands.len());
    Ok(longhands
        .into_iter()
        .zip(values)
        .map(|(id, (value, implicit))| (Declaration { id, value }, implicit))
        .collect())
}

/// One to four values in `top right bottom left` shorthand order, parsed
/// as `side`'s longhand.
fn parse_sides<'i>(input: &mut Parser<'i, '_>, side: LonghandId) -> ParseResult<'i, Vec<Value>> {
    let mut values = vec![side.parse_value(input)?];
    while values.len() < 4 {
        match input.try_parse(|i| side.parse_value(i)) {
            Ok(value) => values.push(value),
            Err(_) => break,
        }
    }
    Ok(match &values[..] {
        [all] => vec![all.clone(); 4],
        [v, h] => vec![v.clone(), h.clone(), v.clone(), h.clone()],
        [top, h, bottom] => vec![top.clone(), h.clone(), bottom.clone(), h.clone()],
        _ => values,
    })
}

/// `<line-width> || <line-style> || <color>`: each at most once, in any
/// order; a missing one is reset to its initial value (and marked as left
/// out).
fn parse_border_side<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, [(Value, bool); 3]> {
    let (mut width, mut style, mut color) = (None, None, None);
    loop {
        if width.is_none()
            && let Ok(v) = input.try_parse(|i| BorderTopWidth.parse_value(i))
        {
            width = Some(v);
            continue;
        }
        if style.is_none()
            && let Ok(v) = input.try_parse(|i| BorderTopStyle.parse_value(i))
        {
            style = Some(v);
            continue;
        }
        if color.is_none()
            && let Ok(v) = input.try_parse(|i| BorderTopColor.parse_value(i))
        {
            color = Some(v);
            continue;
        }
        break;
    }
    if width.is_none() && style.is_none() && color.is_none() {
        return Err(input.new_custom_error(()));
    }
    let or_initial = |value: Option<Value>, initial: Value| match value {
        Some(value) => (value, false),
        None => (initial, true),
    };
    Ok([
        or_initial(width, Value::LineWidth(LineWidth::Medium)),
        or_initial(style, Value::BorderStyle(computed_style::BorderStyle::None)),
        or_initial(color, Value::Color(SpecifiedColor::CurrentColor)),
    ])
}

/// `outline`: `<outline-color> || <outline-style> || <outline-width>`, each
/// at most once; a missing one is reset to its initial value (and marked
/// as left out).
/// `text-decoration`: `<'text-decoration-line'> || <'text-decoration-
/// thickness'> || <'text-decoration-style'> || <'text-decoration-color'>`,
/// in `Shorthand::longhands` order.
fn parse_text_decoration<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, [(Value, bool); 4]> {
    let ids = [
        TextDecorationLine,
        TextDecorationThickness,
        TextDecorationStyle,
        TextDecorationColor,
    ];
    let mut values: [Option<Value>; 4] = Default::default();
    'parts: loop {
        for (id, value) in ids.iter().zip(&mut values) {
            if value.is_none()
                && let Ok(v) = input.try_parse(|i| id.parse_value(i))
            {
                *value = Some(v);
                continue 'parts;
            }
        }
        break;
    }
    if values.iter().all(Option::is_none) {
        return Err(input.new_custom_error(()));
    }
    let initial = [
        Value::TextDecorationLine(TextDecorationLine::NONE),
        Value::TextDecorationThickness(crate::text_decor::SpecifiedThickness::Auto),
        Value::TextDecorationStyle(computed_style::TextDecorationStyle::Solid),
        Value::Color(SpecifiedColor::CurrentColor),
    ];
    let mut out = values
        .into_iter()
        .zip(initial)
        .map(|(value, initial)| match value {
            Some(value) => (value, false),
            None => (initial, true),
        });
    Ok(std::array::from_fn(|_| out.next().expect("four")))
}

fn parse_outline<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, [(Value, bool); 3]> {
    let (mut color, mut style, mut width) = (None, None, None);
    loop {
        if color.is_none()
            && let Ok(v) = input.try_parse(|i| OutlineColor.parse_value(i))
        {
            color = Some(v);
            continue;
        }
        if style.is_none()
            && let Ok(v) = input.try_parse(|i| OutlineStyle.parse_value(i))
        {
            style = Some(v);
            continue;
        }
        if width.is_none()
            && let Ok(v) = input.try_parse(|i| OutlineWidth.parse_value(i))
        {
            width = Some(v);
            continue;
        }
        break;
    }
    if color.is_none() && style.is_none() && width.is_none() {
        return Err(input.new_custom_error(()));
    }
    let or_initial = |value: Option<Value>, initial: Value| match value {
        Some(value) => (value, false),
        None => (initial, true),
    };
    Ok([
        or_initial(color, Value::Color(SpecifiedColor::CurrentColor)),
        or_initial(
            style,
            Value::OutlineStyle(computed_style::OutlineStyle::Line(
                computed_style::BorderStyle::None,
            )),
        ),
        or_initial(width, Value::LineWidth(LineWidth::Medium)),
    ])
}

/// The `flex` shorthand: `none` is `0 0 auto`; an omitted grow or shrink
/// factor is 1, and an omitted basis `0%` (css-flexbox 1, "flex"). A
/// unitless zero is a flex factor unless two came before it.
fn parse_flex<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(vec![
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Size(SizeValue::Auto),
        ]);
    }
    let (mut factors, mut basis) = (None, None);
    loop {
        if factors.is_none()
            && let Ok(grow) = input.try_parse(parse_non_negative_number)
        {
            let shrink = input.try_parse(parse_non_negative_number).ok();
            factors = Some((grow, shrink));
            continue;
        }
        if basis.is_none()
            && let Ok(v) = input.try_parse(|i| FlexBasis.parse_value(i))
        {
            basis = Some(v);
            continue;
        }
        break;
    }
    if factors.is_none() && basis.is_none() {
        return Err(input.new_custom_error(()));
    }
    let (grow, shrink) = factors.unwrap_or((1.0, None));
    Ok(vec![
        Value::Number(grow),
        Value::Number(shrink.unwrap_or(1.0)),
        basis.unwrap_or(Value::Size(SizeValue::LengthPercentage(
            LengthPercentage::Percent(0.0),
        ))),
    ])
}

/// The `grid-template` shorthand: `none`; `<'grid-template-rows'> /
/// <'grid-template-columns'>`; or `[ <line-names>? <string> <track-size>?
/// <line-names>? ]+ [ / <explicit-track-list> ]?`, each string a row of
/// `grid-template-areas` (css-grid 2, "grid-template"). The names after
/// a row and before the next name the same line.
fn parse_grid_template<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
    use computed_style::{GridTrack, LineNames, TrackBreadth, TrackList, TrackSize};

    // `none` alone; `none / none` is rows and columns.
    if input
        .try_parse(|i| {
            i.expect_ident_matching("none")?;
            i.expect_exhausted()
        })
        .is_ok()
    {
        return Ok(vec![
            Value::TrackList(TrackList::none()),
            Value::TrackList(TrackList::none()),
            Value::Areas(None),
        ]);
    }
    if let Ok(values) = input.try_parse(|i| {
        let rows = parse_track_list(i)?;
        i.expect_delim('/')?;
        let columns = parse_track_list(i)?;
        Ok::<_, cssparser::ParseError<'i, ()>>(vec![
            Value::TrackList(rows),
            Value::TrackList(columns),
            Value::Areas(None),
        ])
    }) {
        return Ok(values);
    }
    let (mut strings, mut sizes, mut names) = (Vec::new(), Vec::new(), Vec::new());
    // The names after the last row, which the next row's add to.
    let mut after: Vec<Arc<str>> = Vec::new();
    loop {
        let before = input.try_parse(parse_line_names).ok();
        let Ok(string) = input.try_parse(|i| i.expect_string().map(|s| s.to_string())) else {
            if before.is_some() {
                return Err(input.new_custom_error(()));
            }
            break;
        };
        after.extend(before.iter().flat_map(|names| names.iter().cloned()));
        names.push(LineNames::from(std::mem::take(&mut after)));
        strings.push(string);
        sizes.push(GridTrack::Size(
            input
                .try_parse(parse_track_size)
                .unwrap_or(TrackSize::Breadth(TrackBreadth::Auto)),
        ));
        if let Ok(names) = input.try_parse(parse_line_names) {
            after.extend(names.iter().cloned());
        }
    }
    names.push(after.into());
    let areas = areas_from_strings(&strings).ok_or_else(|| input.new_custom_error(()))?;
    let columns = if input.try_parse(|i| i.expect_delim('/')).is_ok() {
        parse_explicit_track_list(input)?
    } else {
        TrackList::none()
    };
    let rows = TrackList {
        tracks: sizes.into(),
        line_names: if names.iter().all(|n| n.is_empty()) {
            Arc::new([])
        } else {
            names.into()
        },
    };
    Ok(vec![
        Value::TrackList(rows),
        Value::TrackList(columns),
        Value::Areas(Some(areas)),
    ])
}

/// The `border-radius` shorthand: the horizontal radii, then those after
/// `/` (or the same), each one to four in corner order and repeated as
/// `margin`'s sides are.
fn parse_border_radius<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<(Value, bool)>> {
    fn radii<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, [LengthPercentage; 4]> {
        let mut values = vec![LengthPercentage::parse(input, false)?];
        while values.len() < 4 {
            match input.try_parse(|i| LengthPercentage::parse(i, false)) {
                Ok(v) => values.push(v),
                Err(_) => break,
            }
        }
        Ok(match &values[..] {
            [all] => [all.clone(), all.clone(), all.clone(), all.clone()],
            [a, b] => [a.clone(), b.clone(), a.clone(), b.clone()],
            [a, b, c] => [a.clone(), b.clone(), c.clone(), b.clone()],
            [a, b, c, d] => [a.clone(), b.clone(), c.clone(), d.clone()],
            _ => unreachable!("one to four"),
        })
    }
    let x = radii(input)?;
    let y = if input.try_parse(|i| i.expect_delim('/')).is_ok() {
        radii(input)?
    } else {
        x.clone()
    };
    Ok((0..4)
        .map(|i| (Value::CornerRadius(x[i].clone(), Some(y[i].clone())), false))
        .collect())
}

/// The `background` shorthand: comma-separated layers, each any of an
/// image, a position (with a size after `/`), a repeat style, an
/// attachment and one or two boxes (one sets both `background-origin` and
/// `background-clip`, two set them in that order), in any order; the last
/// layer may have a color too. What a layer leaves out is reset to its
/// initial value; a longhand left out of every layer is marked as such,
/// as Blink marks each implicit value.
fn parse_background<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<(Value, bool)>> {
    use computed_style::{BackgroundAttachment as Attachment, BackgroundBox as Box};
    #[derive(Default)]
    struct Layer {
        image: Option<SpecifiedImage>,
        position: Option<SpecifiedPosition>,
        size: Option<SpecifiedBackgroundSize>,
        repeat: Option<(
            computed_style::BackgroundRepeat,
            computed_style::BackgroundRepeat,
        )>,
        attachment: Option<Attachment>,
        boxes: Vec<Box>,
        color: Option<SpecifiedColor>,
    }
    let mut layers: Vec<Layer> = Vec::new();
    let mut last = false;
    while !last {
        if !layers.is_empty() {
            input.expect_comma()?;
        }
        let mut layer = Layer::default();
        let mut any = false;
        loop {
            if layer.image.is_none()
                && let Ok(v) = input.try_parse(parse_image)
            {
                layer.image = Some(v);
            } else if layer.position.is_none()
                && let Ok(v) = input.try_parse(SpecifiedPosition::parse)
            {
                layer.position = Some(v);
                if input.try_parse(|i| i.expect_delim('/')).is_ok() {
                    layer.size = Some(SpecifiedBackgroundSize::parse(input)?);
                }
            } else if layer.repeat.is_none()
                && let Ok(v) = input.try_parse(parse_repeat)
            {
                layer.repeat = Some(v);
            } else if layer.attachment.is_none()
                && let Ok(v) = input.try_parse(parse_attachment)
            {
                layer.attachment = Some(v);
            } else if layer.boxes.len() < 2
                && let Ok(v) = input.try_parse(parse_box)
            {
                layer.boxes.push(v);
            } else if layer.color.is_none()
                && let Ok(v) = input.try_parse(parse_style_color)
            {
                layer.color = Some(v);
            } else {
                break;
            }
            any = true;
        }
        if !any {
            return Err(input.new_custom_error(()));
        }
        // Only the last layer may have a color.
        last = input.is_exhausted();
        if layer.color.is_some() && !last {
            return Err(input.new_custom_error(()));
        }
        layers.push(layer);
    }
    fn list<T: Clone>(
        layers: &[Layer],
        get: impl Fn(&Layer) -> Option<T>,
        initial: T,
    ) -> (Arc<[T]>, bool) {
        let implicit = layers.iter().all(|layer| get(layer).is_none());
        let values = layers
            .iter()
            .map(|layer| get(layer).unwrap_or_else(|| initial.clone()))
            .collect();
        (values, implicit)
    }
    use crate::background::SpecifiedPositionComponent as Component;
    let start = Component::Offset(LengthPercentage::Percent(0.0));
    let (images, images_implicit) = list(&layers, |l| l.image.clone(), SpecifiedImage::None);
    let (x, x_implicit) = list(
        &layers,
        |l| l.position.as_ref().map(|p| p.x.clone()),
        start.clone(),
    );
    let (y, y_implicit) = list(&layers, |l| l.position.as_ref().map(|p| p.y.clone()), start);
    let (sizes, sizes_implicit) = list(&layers, |l| l.size.clone(), SpecifiedBackgroundSize::AUTO);
    let repeat = (
        computed_style::BackgroundRepeat::Repeat,
        computed_style::BackgroundRepeat::Repeat,
    );
    let (repeats, repeats_implicit) = list(&layers, |l| l.repeat, repeat);
    let (attachments, attachments_implicit) = list(&layers, |l| l.attachment, Attachment::Scroll);
    let (origins, origins_implicit) = list(&layers, |l| l.boxes.first().copied(), Box::PaddingBox);
    let (clips, clips_implicit) = list(&layers, |l| l.boxes.last().copied(), Box::BorderBox);
    let color = layers.last().and_then(|layer| layer.color.clone());
    let color_implicit = color.is_none();
    Ok(vec![
        (Value::BackgroundImages(images), images_implicit),
        (Value::PositionsX(x), x_implicit),
        (Value::PositionsY(y), y_implicit),
        (Value::BackgroundSizes(sizes), sizes_implicit),
        (Value::BackgroundRepeats(repeats), repeats_implicit),
        (
            Value::BackgroundAttachments(attachments),
            attachments_implicit,
        ),
        (Value::BackgroundBoxes(origins), origins_implicit),
        (Value::BackgroundBoxes(clips), clips_implicit),
        (
            // Left out, it reads back as `rgba(0, 0, 0, 0)`, as in Chrome.
            Value::Color(color.unwrap_or(SpecifiedColor::Rgba(gfx::Color::TRANSPARENT))),
            color_implicit,
        ),
    ])
}

/// The `font` shorthand. `normal` may stand for any of the optional parts;
/// omitted ones are reset. A system font sets them all pending.
fn parse_font<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
    // The values but style, weight, size, line height and family, which
    // `font` resets: its small caps and stretch in the caller's.
    let variant = crate::font_features::variant_initial_values();
    let others = crate::font_features::other_initial_values();
    let stretch =
        |stretch| Value::FontStretch(crate::font_features::SpecifiedFontStretch::Keyword(stretch));
    if let Ok(system) = input.try_parse(crate::font::parse_system_font) {
        let system = Value::SystemFont(system);
        return Ok([vec![system.clone()], variant.to_vec(), others.to_vec()]
            .concat()
            .into_iter()
            .chain([
                system.clone(),
                stretch(computed_style::FontStretch::Normal),
                system.clone(),
                system.clone(),
                system,
            ])
            .collect());
    }
    let font = crate::font::parse_font_shorthand(input)?;
    let mut variant = variant.to_vec();
    if font.small_caps {
        variant[3] = Value::FontVariantCaps(computed_style::FontVariantCaps::SmallCaps);
    }
    Ok([
        vec![Value::FontStyle(
            font.style.unwrap_or(computed_style::FontStyle::Normal),
        )],
        variant,
        others.to_vec(),
        vec![
            Value::FontWeight(font.weight.unwrap_or(FontWeightValue::Normal)),
            stretch(font.stretch.unwrap_or(computed_style::FontStretch::Normal)),
            Value::FontSize(font.size),
            Value::LineHeight(font.line_height),
            Value::FontFamily(font.family),
        ],
    ]
    .concat())
}

/// Sets a font property, leaving the font group shared while the value is
/// the one it has (most elements' `font` shorthand and `font: inherit`
/// set them to what they inherit).
macro_rules! set_font {
    ($style:expr, $field:ident, $value:expr) => {{
        let value = $value;
        if $style.font.$field != value {
            $style.font_mut().$field = value;
        }
    }};
}

/// Sets the rare group's `$field` to `$value` unless it is that already:
/// `border` resets `border-image` on every element it is set on, and each
/// element that writes the group gets its own copy.
macro_rules! set_rare {
    ($style:expr, $($field:ident).+, $value:expr) => {{
        let value = $value;
        if $style.rare.$($field).+ != value {
            $style.rare_mut().$($field).+ = value;
        }
    }};
}

/// Applies one declaration on top of `style`.
pub fn apply(
    style: &mut ComputedStyle,
    declaration: &Declaration,
    parent: &ComputedStyle,
    cx: &ComputeContext,
) {
    // A flow-relative longhand sets its physical one.
    let id = declaration.id.physical();
    if let Value::Unparsed(_) = declaration.value {
        unreachable!("{id:?}'s value is substituted before it is applied");
    }
    if let Value::CssWide(keyword) = declaration.value {
        let inherit = match keyword {
            CssWideKeyword::Inherit => true,
            CssWideKeyword::Initial => false,
            CssWideKeyword::Unset => id.is_inherited(),
        };
        // A child of it takes what it has of its parent's non-inherited
        // values again when they change.
        if inherit && !id.is_inherited() {
            style.explicit_inheritance = true;
        }
        if inherit {
            copy(id, parent, style);
        } else {
            INITIAL.with(|initial| copy(id, initial, style));
        }
        return;
    }
    let auto_or = |v: Option<LengthPercentage>| match v {
        None => LengthPercentageOrAuto::Auto,
        Some(lp) => LengthPercentageOrAuto::LengthPercentage(lp.compute(cx)),
    };
    let side = |ids: [LonghandId; 4]| ids.iter().position(|&i| i == id).unwrap();
    match (id, declaration.value.clone()) {
        (Display, Value::Display(v)) => style.display = v,
        (BoxSizing, Value::BoxSizing(v)) => style.box_sizing = v,
        (Position, Value::Position(v)) => style.position = v,
        (Float, Value::Float(v)) => style.float = v,
        (Clear, Value::Clear(v)) => style.clear = v,
        (Content, Value::Content(v)) => style.rare_mut().content = v,
        (CounterReset, Value::CounterReset(v)) => style.rare_mut().counter_reset = v,
        (CounterIncrement, Value::CounterChanges(v)) => style.rare_mut().counter_increment = v,
        (CounterSet, Value::CounterChanges(v)) => style.rare_mut().counter_set = v,
        (ListStyleType, Value::ListStyleType(v)) => style.rare_inherited_mut().list_style_type = v,
        (ListStylePosition, Value::ListStylePosition(v)) => style.list_style_position = v,
        (ListStyleImage, Value::ListStyleImage) => {}
        (Top | Right | Bottom | Left, Value::AutoOr(v)) => {
            *side_mut(&mut style.inset, side(INSET)) = auto_or(v);
        }
        (Width, Value::Size(v)) => style.width = v.compute(cx),
        (Height, Value::Size(v)) => style.height = v.compute(cx),
        (MinWidth, Value::AutoOr(v)) => style.min_width = auto_or(v),
        (MinHeight, Value::AutoOr(v)) => style.min_height = auto_or(v),
        (MaxWidth | MaxHeight, Value::NoneOr(v)) => {
            let v = match v {
                None => LengthPercentageOrNone::None,
                Some(lp) => LengthPercentageOrNone::LengthPercentage(lp.compute(cx)),
            };
            if id == MaxWidth {
                style.max_width = v;
            } else {
                style.max_height = v;
            }
        }
        (MarginTop | MarginRight | MarginBottom | MarginLeft, Value::AutoOr(v)) => {
            *side_mut(&mut style.margin, side(MARGIN)) = auto_or(v);
        }
        (PaddingTop | PaddingRight | PaddingBottom | PaddingLeft, Value::LengthPercentage(v)) => {
            *side_mut(&mut style.padding, side(PADDING)) = v.compute(cx);
        }
        (
            BorderTopWidth | BorderRightWidth | BorderBottomWidth | BorderLeftWidth,
            Value::LineWidth(v),
        ) => {
            // Zeroed later if the style ends up `none` (`finish`).
            *side_mut(&mut style.border_width, side(BORDER_WIDTH)) = v.to_px(cx);
        }
        (
            BorderTopStyle | BorderRightStyle | BorderBottomStyle | BorderLeftStyle,
            Value::BorderStyle(v),
        ) => {
            *side_mut(&mut style.border_style, side(BORDER_STYLE)) = v;
        }
        (
            BorderTopColor | BorderRightColor | BorderBottomColor | BorderLeftColor,
            Value::Color(v),
        ) => {
            *side_mut(&mut style.border_color, side(BORDER_COLOR)) = v.compute(cx);
        }
        (
            BorderTopLeftRadius
            | BorderTopRightRadius
            | BorderBottomRightRadius
            | BorderBottomLeftRadius,
            Value::CornerRadius(x, y),
        ) => {
            let x = x.compute(cx);
            let radius = computed_style::CornerRadius {
                x,
                y: y.map_or(x, |y| y.compute(cx)),
            };
            *corner_mut(&mut style.rare_mut().border_radius, corner(id)) = radius;
        }
        (BoxShadow, Value::BoxShadows(v)) => style.box_shadow = compute_shadows(&v, cx),
        // Zeroed later if the style ends up `none` (`finish`).
        (OutlineWidth, Value::LineWidth(v)) => style.outline_width = v.to_px(cx),
        (OutlineStyle, Value::OutlineStyle(v)) => style.outline_style = v,
        (OutlineColor, Value::Color(v)) => style.outline_color = v.compute(cx),
        (OutlineOffset, Value::Length(v)) => style.outline_offset = v.to_px(cx),
        (BackgroundColor, Value::Color(v)) => style.background_color = v.compute(cx),
        (BackgroundImage, Value::BackgroundImages(v)) => {
            style.backgrounds_mut().background_image =
                v.iter().map(|image| image.compute(cx)).collect()
        }
        (BackgroundPositionX, Value::PositionsX(v)) => {
            style.backgrounds_mut().background_position_x =
                v.iter().map(|c| c.compute(cx)).collect()
        }
        (BackgroundPositionY, Value::PositionsY(v)) => {
            style.backgrounds_mut().background_position_y =
                v.iter().map(|c| c.compute(cx)).collect()
        }
        (BackgroundSize, Value::BackgroundSizes(v)) => {
            style.backgrounds_mut().background_size =
                v.iter().map(|size| size.compute(cx)).collect()
        }
        (BackgroundRepeat, Value::BackgroundRepeats(v)) => {
            style.backgrounds_mut().background_repeat = v
        }
        (BackgroundAttachment, Value::BackgroundAttachments(v)) => {
            style.backgrounds_mut().background_attachment = v
        }
        (BackgroundClip, Value::BackgroundBoxes(v)) => style.backgrounds_mut().background_clip = v,
        (BackgroundOrigin, Value::BackgroundBoxes(v)) => {
            style.backgrounds_mut().background_origin = v
        }
        // `color: currentcolor` means the inherited color.
        (Color, Value::Color(v)) => style.color = parent.resolve(v.compute(cx)),
        (TextDecorationLine, Value::TextDecorationLine(v)) => style.text_decoration_line = v,
        (TextDecorationStyle, Value::TextDecorationStyle(v)) => {
            style.rare_mut().text_decoration_style = v
        }
        (TextDecorationColor, Value::Color(v)) => {
            style.rare_mut().text_decoration_color = v.compute(cx)
        }
        (TextDecorationThickness, Value::TextDecorationThickness(v)) => {
            style.rare_mut().text_decoration_thickness = v.compute(cx)
        }
        (TextUnderlineOffset, Value::AutoOr(v)) => {
            style.rare_inherited_mut().text_underline_offset = v.map(|v| v.compute(cx))
        }
        (TextUnderlinePosition, Value::TextUnderlinePosition(v)) => {
            style.rare_inherited_mut().text_underline_position = v
        }
        (TextShadow, Value::BoxShadows(v)) => {
            style.rare_inherited_mut().text_shadow = compute_shadows(&v, cx)
        }
        (WebkitTextFillColor, Value::Color(v)) => {
            style.rare_inherited_mut().text_fill_color = v.compute(cx)
        }
        (Filter, Value::Filter(v)) => {
            style.rare_mut().filter = crate::filter::compute_filter(&v, cx)
        }
        (Isolation, Value::Isolation(v)) => style.rare_mut().isolation = v,
        (Opacity, Value::Opacity(v)) => style.opacity = v.clamp(0.0, 1.0),
        (OverflowX, Value::Overflow(v)) => style.overflow_x = v.compute(),
        (OverflowY, Value::Overflow(v)) => style.overflow_y = v.compute(),
        (Transform, Value::Transform(v)) => style.transform = compute_transform(&v, cx),
        (TransformOrigin, Value::TransformOrigin(v)) => {
            style.rare_mut().transform_origin = compute_transform_origin(&v, cx)
        }
        (TransformBox, Value::TransformBox(v)) => style.rare_mut().transform_box = v,
        (ObjectFit, Value::ObjectFit(v)) => style.rare_mut().object_fit = v,
        (ObjectPosition, Value::ObjectPosition(v)) => {
            style.rare_mut().object_position = v.compute(cx)
        }
        (WebkitBoxOrient, Value::BoxOrient(v)) => style.rare_mut().box_orient = v,
        (WebkitBoxDirection, Value::BoxDirection(v)) => style.rare_mut().box_direction = v,
        (WebkitBoxPack, Value::BoxPack(v)) => style.rare_mut().box_pack = v,
        (WebkitBoxAlign, Value::BoxAlign(v)) => style.rare_mut().box_align = v,
        (WebkitBoxFlex, Value::Number(v)) => style.rare_mut().box_flex = v,
        // Both at least 1 as parsed.
        (WebkitBoxOrdinalGroup, Value::Integer(v)) => {
            style.rare_mut().box_ordinal_group = v.unsigned_abs()
        }
        (WebkitLineClamp, Value::LineClamp(v)) => {
            style.rare_mut().line_clamp = v.map(i32::unsigned_abs)
        }
        (Contain, Value::Contain(v)) => style.rare_mut().contain = v.compute(),
        (Clip, Value::Clip(v)) => style.rare_mut().clip = crate::masking::compute_clip(&v, cx),
        (ClipPath, Value::ClipPath(v)) => {
            style.rare_mut().clip_path = crate::masking::compute_clip_path(&v, cx)
        }
        (FlexDirection, Value::FlexDirection(v)) => style.flex_direction = v,
        (FlexWrap, Value::FlexWrap(v)) => style.flex_wrap = v,
        (FlexGrow, Value::Number(v)) => style.flex_grow = v,
        (FlexShrink, Value::Number(v)) => style.flex_shrink = v,
        (FlexBasis, Value::Size(v)) => {
            style.flex_basis = match v {
                SizeValue::Content => computed_style::FlexBasis::Content,
                v => computed_style::FlexBasis::Size(v.compute(cx)),
            }
        }
        (Order, Value::Integer(v)) => style.order = v,
        (JustifyContent, Value::ContentAlignment(v)) => style.justify_content = v,
        (AlignContent, Value::ContentAlignment(v)) => style.align_content = v,
        (AlignItems, Value::ItemAlignment(v)) => style.align_items = v,
        (AlignSelf, Value::AlignSelf(v)) => style.align_self = v,
        // `normal` is 0 in flex and grid layout, the only ones with gaps
        // so far.
        (RowGap, Value::Gap(v)) => style.row_gap = v.map_or(ZERO_GAP, |v| v.compute(cx)),
        (ColumnGap, Value::Gap(v)) => style.column_gap = v.map_or(ZERO_GAP, |v| v.compute(cx)),
        (ZIndex, Value::AutoOrInteger(v)) => style.z_index = v,
        (GridTemplateColumns, Value::TrackList(v)) => {
            style.rare_mut().grid_template_columns = compute_track_list(&v, cx)
        }
        (GridTemplateRows, Value::TrackList(v)) => {
            style.rare_mut().grid_template_rows = compute_track_list(&v, cx)
        }
        (GridTemplateAreas, Value::Areas(v)) => style.rare_mut().grid_template_areas = v,
        (GridAutoColumns, Value::TrackSizes(v)) => {
            style.rare_mut().grid_auto_columns = compute_track_sizes(&v, cx)
        }
        (GridAutoRows, Value::TrackSizes(v)) => {
            style.rare_mut().grid_auto_rows = compute_track_sizes(&v, cx)
        }
        (GridAutoFlow, Value::GridAutoFlow(v)) => style.rare_mut().grid_auto_flow = v,
        (GridRowStart, Value::GridLine(v)) => style.rare_mut().grid_row_start = v,
        (GridRowEnd, Value::GridLine(v)) => style.rare_mut().grid_row_end = v,
        (GridColumnStart, Value::GridLine(v)) => style.rare_mut().grid_column_start = v,
        (GridColumnEnd, Value::GridLine(v)) => style.rare_mut().grid_column_end = v,
        (JustifyItems, Value::ItemAlignment(v)) => style.justify_items = v,
        (JustifySelf, Value::AlignSelf(v)) => style.justify_self = v,
        (FontFamily, Value::FontFamily(v)) => set_font!(style, family, v),
        (FontFamily, Value::SystemFont(_)) => set_font!(
            style,
            family,
            Arc::new([computed_style::FontFamily::Generic(
                computed_style::GenericFamily::SystemUi,
            )]) as Arc<[_]>
        ),
        (FontSize, Value::SystemFont(v)) => set_font!(style, size, v.size()),
        (FontWeight, Value::SystemFont(_)) => set_font!(style, weight, 400.0),
        (FontStyle, Value::SystemFont(_)) => {
            set_font!(style, style, computed_style::FontStyle::Normal)
        }
        (LineHeight, Value::SystemFont(_)) => {
            style.line_height = computed_style::LineHeight::Normal
        }
        (FontSize, Value::FontSize(v)) => set_font!(style, size, v.compute(cx)),
        (FontWeight, Value::FontWeight(v)) => {
            set_font!(style, weight, v.compute(parent.font.weight))
        }
        (FontStyle, Value::FontStyle(v)) => set_font!(style, style, v),
        (FontStretch, Value::FontStretch(v)) => set_font!(style, stretch, v.percentage()),
        (FontVariantLigatures, Value::FontVariantLigatures(v)) => {
            set_font!(style, variant_ligatures, v)
        }
        (FontVariantNumeric, Value::FontVariantNumeric(v)) => set_font!(style, variant_numeric, v),
        (FontVariantEastAsian, Value::FontVariantEastAsian(v)) => {
            set_font!(style, variant_east_asian, v)
        }
        (FontVariantCaps, Value::FontVariantCaps(v)) => set_font!(style, variant_caps, v),
        (FontVariantAlternates, Value::FontVariantAlternates(v)) => {
            set_font!(style, variant_alternates, v)
        }
        (FontVariantPosition, Value::FontVariantPosition(v)) => {
            set_font!(style, variant_position, v)
        }
        (FontVariantEmoji, Value::FontVariantEmoji(v)) => set_font!(style, variant_emoji, v),
        (FontSizeAdjust, Value::FontSizeAdjust(v)) => set_font!(style, size_adjust, v),
        (FontLanguageOverride, Value::FontLanguageOverride(v)) => {
            set_font!(style, language_override, v)
        }
        (FontKerning, Value::FontKerning(v)) => set_font!(style, kerning, v),
        (FontOpticalSizing, Value::FontOpticalSizing(v)) => set_font!(style, optical_sizing, v),
        (FontFeatureSettings, Value::FontFeatureSettings(v)) => set_font!(
            style,
            feature_settings,
            crate::font_features::computed_settings(&v, |s| s.tag)
        ),
        (FontVariationSettings, Value::FontVariationSettings(v)) => set_font!(
            style,
            variation_settings,
            crate::font_features::computed_settings(&v, |s| s.tag)
        ),
        (FontSynthesisWeight, Value::FontSynthesis(v)) => set_font!(style, synthesis_weight, v),
        (FontSynthesisStyle, Value::FontSynthesis(v)) => set_font!(style, synthesis_style, v),
        (FontSynthesisSmallCaps, Value::FontSynthesis(v)) => {
            set_font!(style, synthesis_small_caps, v)
        }
        (LineHeight, Value::LineHeight(v)) => style.line_height = v.compute(cx),
        (TextAlign, Value::TextAlign(v)) => style.text_align = v,
        (VerticalAlign, Value::VerticalAlign(v)) => style.vertical_align = v,
        (VerticalAlign, Value::LengthPercentage(v)) => {
            style.vertical_align = computed_style::VerticalAlign::Length(v.compute(cx));
        }
        (WhiteSpaceCollapse, Value::WhiteSpaceCollapse(v)) => style.white_space_collapse = v,
        (TextWrapMode, Value::TextWrapMode(v)) => style.text_wrap_mode = v,
        (TextTransform, Value::TextTransform(v)) => style.text_transform = v,
        (TextOverflow, Value::TextOverflow(v)) => style.text_overflow = v,
        (Appearance, Value::Appearance(v)) => style.appearance = v,
        // 0 is `normal`, as Chrome computes it.
        (LetterSpacing, Value::Spacing(v)) => {
            style.rare_inherited_mut().letter_spacing =
                v.map(|l| l.to_px(cx)).filter(|px| *px != 0.0);
        }
        (WordSpacing, Value::Spacing(v)) => {
            style.rare_inherited_mut().word_spacing = v.map_or(0.0, |l| l.to_px(cx))
        }
        (TabSize, Value::TabSize(v)) => {
            style.rare_inherited_mut().tab_size = match v {
                SpecifiedTabSize::Spaces(spaces) => computed_style::TabSize::Spaces(spaces),
                SpecifiedTabSize::Length(length) => computed_style::TabSize::Px(length.to_px(cx)),
            }
        }
        (TextIndent, Value::LengthPercentage(v)) => {
            style.rare_inherited_mut().text_indent = v.compute(cx)
        }
        (WordBreak, Value::WordBreak(v)) => style.rare_inherited_mut().word_break = v,
        (OverflowWrap, Value::OverflowWrap(v)) => style.rare_inherited_mut().overflow_wrap = v,
        (Fill, Value::Paint(v)) => style.svg_mut().fill = v.compute(cx),
        (Stroke, Value::Paint(v)) => style.svg_mut().stroke = v.compute(cx),
        (FillOpacity, Value::Opacity(v)) => style.svg_mut().fill_opacity = v.clamp(0.0, 1.0),
        (StrokeOpacity, Value::Opacity(v)) => style.svg_mut().stroke_opacity = v.clamp(0.0, 1.0),
        (StopColor, Value::Color(v)) => style.svg_mut().stop_color = v.compute(cx),
        (StopOpacity, Value::Opacity(v)) => style.svg_mut().stop_opacity = v.clamp(0.0, 1.0),
        (FillRule, Value::FillRule(v)) => style.svg_mut().fill_rule = v,
        (ClipRule, Value::FillRule(v)) => style.svg_mut().clip_rule = v,
        (TextAnchor, Value::TextAnchor(v)) => style.svg_mut().text_anchor = v,
        (StrokeWidth, Value::LengthPercentage(v)) => style.svg_mut().stroke_width = v.compute(cx),
        (StrokeLinecap, Value::StrokeLinecap(v)) => style.svg_mut().stroke_linecap = v,
        (StrokeLinejoin, Value::StrokeLinejoin(v)) => style.svg_mut().stroke_linejoin = v,
        (StrokeMiterlimit, Value::Number(v)) => style.svg_mut().stroke_miterlimit = v,
        (StrokeDasharray, Value::Dasharray(v)) => {
            style.svg_mut().stroke_dasharray = match v {
                None => Arc::new([]),
                Some(dashes) => dashes.iter().map(|dash| dash.compute(cx)).collect(),
            }
        }
        (StrokeDashoffset, Value::LengthPercentage(v)) => {
            style.svg_mut().stroke_dashoffset = v.compute(cx)
        }
        (Cx, Value::LengthPercentage(v)) => style.svg_mut().cx = v.compute(cx),
        (Cy, Value::LengthPercentage(v)) => style.svg_mut().cy = v.compute(cx),
        (R, Value::LengthPercentage(v)) => style.svg_mut().r = v.compute(cx),
        (Rx, Value::AutoOr(v)) => style.svg_mut().rx = auto_or(v),
        (Ry, Value::AutoOr(v)) => style.svg_mut().ry = auto_or(v),
        (X, Value::LengthPercentage(v)) => style.svg_mut().x = v.compute(cx),
        (Y, Value::LengthPercentage(v)) => style.svg_mut().y = v.compute(cx),
        (D, Value::Path(v)) => style.svg_mut().d = v.map(|segments| absolute_path(&segments)),
        (Visibility, Value::Visibility(v)) => style.visibility = v,
        (WebkitTextSecurity, Value::TextSecurity(v)) => {
            style.rare_inherited_mut().text_security = v
        }
        (PointerEvents, Value::PointerEvents(v)) => style.pointer_events = v,
        (BorderCollapse, Value::BorderCollapse(v)) => style.border_collapse = v,
        (WebkitBorderHorizontalSpacing, Value::Length(v)) => {
            style.rare_inherited_mut().border_spacing.horizontal = v.to_px(cx)
        }
        (WebkitBorderVerticalSpacing, Value::Length(v)) => {
            style.rare_inherited_mut().border_spacing.vertical = v.to_px(cx)
        }
        (TableLayout, Value::TableLayout(v)) => style.table_layout = v,
        (CaptionSide, Value::CaptionSide(v)) => style.caption_side = v,
        (EmptyCells, Value::EmptyCells(v)) => style.empty_cells = v,
        (UserSelect, Value::UserSelect(v)) => style.rare_inherited_mut().user_select = v,
        (WebkitUserModify, Value::UserModify(v)) => style.rare_inherited_mut().user_modify = v,
        (Hyphens, Value::Hyphens(v)) => style.rare_inherited_mut().hyphens = v,
        (ForcedColorAdjust, Value::ForcedColorAdjust(v)) => {
            style.rare_inherited_mut().forced_color_adjust = v
        }
        (WebkitTapHighlightColor, Value::Color(v)) => {
            style.rare_inherited_mut().tap_highlight_color = v.compute(cx)
        }
        (CaretColor, Value::CaretColor(v)) => {
            style.rare_inherited_mut().caret_color = match v {
                None => computed_style::CaretColor::Auto,
                Some(color) => computed_style::CaretColor::Color(color.compute(cx)),
            }
        }
        (TextSizeAdjust, Value::TextSizeAdjust(v)) => {
            style.rare_inherited_mut().text_size_adjust = match v {
                SpecifiedTextSizeAdjust::Auto => computed_style::TextSizeAdjust::Auto,
                // Chrome computes `none` to 100%.
                SpecifiedTextSizeAdjust::None => computed_style::TextSizeAdjust::Percent(100.0),
                SpecifiedTextSizeAdjust::Percent(v) => computed_style::TextSizeAdjust::Percent(v),
            }
        }
        (TextRendering, Value::TextRendering(v)) => {
            set_font!(style, text_rendering, v)
        }
        (WebkitFontSmoothing, Value::FontSmoothing(v)) => {
            set_font!(style, smoothing, v)
        }
        (Resize, Value::Resize(v)) => style.rare_mut().resize = v,
        (ScrollBehavior, Value::ScrollBehavior(v)) => style.rare_mut().scroll_behavior = v,
        (OverflowAnchor, Value::OverflowAnchor(v)) => style.rare_mut().overflow_anchor = v,
        (Cursor, Value::Cursor(v)) => {
            let cursor = v.compute();
            if style.rare_inherited.cursor != cursor {
                style.rare_inherited_mut().cursor = cursor;
            }
        }
        (BorderImageSource, Value::BorderImageSource(v)) => {
            set_rare!(style, border_image.source, v.compute(cx))
        }
        (BorderImageSlice, Value::BorderImageSlice(v)) => {
            set_rare!(style, border_image.slice, v)
        }
        (BorderImageWidth, Value::BorderImageWidth(v)) => set_rare!(
            style,
            border_image.width,
            crate::border_image::compute_width(&v, cx)
        ),
        (BorderImageOutset, Value::BorderImageOutset(v)) => set_rare!(
            style,
            border_image.outset,
            crate::border_image::compute_outset(&v, cx)
        ),
        (BorderImageRepeat, Value::BorderImageRepeat(horizontal, vertical)) => {
            set_rare!(style, border_image.repeat, (horizontal, vertical))
        }
        (TransitionProperty, Value::TransitionProperties(v)) => style.transitions.property = v,
        (TransitionDuration, Value::Times(v)) => {
            style.transitions.duration = v.iter().map(|t| t.seconds()).collect();
        }
        (TransitionTimingFunction, Value::Easings(v)) => {
            style.transitions.timing_function = v.iter().map(SpecifiedEasing::compute).collect();
        }
        (TransitionDelay, Value::Times(v)) => {
            style.transitions.delay = v.iter().map(|t| t.seconds()).collect();
        }
        (TransitionBehavior, Value::TransitionBehaviors(v)) => style.transitions.behavior = v,
        (AnimationDuration, Value::AnimationDurations(v)) => {
            style.rare_mut().animations.duration =
                v.iter().map(|t| t.map_or(0.0, |t| t.seconds())).collect();
        }
        (AnimationTimingFunction, Value::Easings(v)) => {
            style.rare_mut().animations.timing_function =
                v.iter().map(SpecifiedEasing::compute).collect();
        }
        (AnimationDelay, Value::Times(v)) => {
            style.rare_mut().animations.delay = v.iter().map(|t| t.seconds()).collect();
        }
        (AnimationIterationCount, Value::IterationCounts(v)) => {
            style.rare_mut().animations.iteration_count = v;
        }
        (AnimationDirection, Value::PlaybackDirections(v)) => {
            style.rare_mut().animations.direction = v
        }
        (AnimationFillMode, Value::FillModes(v)) => style.rare_mut().animations.fill_mode = v,
        (AnimationPlayState, Value::PlayStates(v)) => style.rare_mut().animations.play_state = v,
        (AnimationName, Value::AnimationNames(v)) => style.rare_mut().animations.name = v,
        (AnimationTimeline, Value::AnimationTimelines(v)) => {
            style.rare_mut().animations.timeline = v
        }
        (AnimationRangeStart, Value::AnimationRangeBoundaries(v)) => {
            style.rare_mut().animations.range_start = v
        }
        (AnimationRangeEnd, Value::AnimationRangeBoundaries(v)) => {
            style.rare_mut().animations.range_end = v
        }
        (_, value) => unreachable!("{value:?} is not a value of {id:?}"),
    }
}

const ZERO_GAP: computed_style::LengthPercentage = computed_style::LengthPercentage::Px(0.0);

/// What the cascade can't do one declaration at a time: a border side
/// whose style is `none` or `hidden` has a computed width of 0 (not an outline's,
/// which css-ui 4 and Chrome keep), and the element's text
/// decorations add to the ones it inherited.
pub fn finish(style: &mut ComputedStyle, parent: &ComputedStyle) {
    // A scroll container scrolls both axes: next to an axis that scrolls,
    // `visible` computes to `auto` and `clip` to `hidden` (css-overflow 3,
    // "overflow-x / overflow-y"). `visible` and `clip` together clip one
    // axis only.
    use computed_style::Overflow;
    let scrolls =
        |axis: Overflow| matches!(axis, Overflow::Hidden | Overflow::Auto | Overflow::Scroll);
    if scrolls(style.overflow_x) || scrolls(style.overflow_y) {
        for axis in [&mut style.overflow_x, &mut style.overflow_y] {
            *axis = match *axis {
                Overflow::Visible => Overflow::Auto,
                Overflow::Clip => Overflow::Hidden,
                scrolling => scrolling,
            };
        }
    }
    // Written only when it isn't the parent's, which the group came with:
    // most elements share their parent's group whole.
    let decorations =
        style.applied_text_decorations_from(&parent.rare_inherited.applied_text_decorations);
    if !Arc::ptr_eq(&decorations, &style.rare_inherited.applied_text_decorations) {
        style.rare_inherited_mut().applied_text_decorations = decorations;
    }
    for i in 0..4 {
        if !side_mut(&mut style.border_style, i).is_drawn() {
            *side_mut(&mut style.border_width, i) = 0.0;
        }
    }
}

thread_local! {
    /// What an element whose parent has every initial value starts the
    /// cascade with: where `initial` takes each property's value from.
    static INITIAL: ComputedStyle = cascade_start(&ComputedStyle::initial());
}

/// Where cascading an element with parent style `parent` starts: its
/// inherited properties, the others initial. The border and outline
/// widths start at `medium`, their initial value: [`finish`] zeroes the
/// borders whose style ends up `none`, which is what the initial computed
/// style (`ComputedStyle::initial`) has.
pub fn cascade_start(parent: &ComputedStyle) -> ComputedStyle {
    // `medium`'s width, which depends on nothing.
    let medium = 3.0;
    ComputedStyle {
        border_width: computed_style::Sides::all(medium),
        outline_width: medium,
        ..ComputedStyle::inherit(parent)
    }
}

/// Which corner `id` is, in `border-radius` order.
fn corner(id: LonghandId) -> usize {
    BORDER_RADIUS
        .iter()
        .position(|&i| i == id)
        .expect("a corner's longhand")
}

/// Corner `i` in `border-radius` order.
fn corner_ref<T>(corners: &computed_style::Corners<T>, i: usize) -> &T {
    match i {
        0 => &corners.top_left,
        1 => &corners.top_right,
        2 => &corners.bottom_right,
        3 => &corners.bottom_left,
        _ => unreachable!(),
    }
}

/// Corner `i` in `border-radius` order.
fn corner_mut<T>(corners: &mut computed_style::Corners<T>, i: usize) -> &mut T {
    match i {
        0 => &mut corners.top_left,
        1 => &mut corners.top_right,
        2 => &mut corners.bottom_right,
        3 => &mut corners.bottom_left,
        _ => unreachable!(),
    }
}

/// Side `i` in `top right bottom left` order.
fn side<T>(sides: &computed_style::Sides<T>, i: usize) -> &T {
    match i {
        0 => &sides.top,
        1 => &sides.right,
        2 => &sides.bottom,
        3 => &sides.left,
        _ => unreachable!(),
    }
}

/// Side `i` in `top right bottom left` order.
fn side_mut<T>(sides: &mut computed_style::Sides<T>, i: usize) -> &mut T {
    match i {
        0 => &mut sides.top,
        1 => &mut sides.right,
        2 => &mut sides.bottom,
        3 => &mut sides.left,
        _ => unreachable!(),
    }
}

/// Copies one property's computed value: a physical longhand's (`apply`
/// maps a flow-relative one first).
fn copy(id: LonghandId, from: &ComputedStyle, to: &mut ComputedStyle) {
    let side_of = |ids: [LonghandId; 4]| ids.iter().position(|&i| i == id);
    match id {
        Display => to.display = from.display,
        BoxSizing => to.box_sizing = from.box_sizing,
        Position => to.position = from.position,
        Float => to.float = from.float,
        Clear => to.clear = from.clear,
        Content => to.rare_mut().content = from.rare.content.clone(),
        CounterReset => to.rare_mut().counter_reset = from.rare.counter_reset.clone(),
        CounterIncrement => to.rare_mut().counter_increment = from.rare.counter_increment.clone(),
        CounterSet => to.rare_mut().counter_set = from.rare.counter_set.clone(),
        ListStyleType => {
            to.rare_inherited_mut().list_style_type = from.rare_inherited.list_style_type.clone()
        }
        ListStylePosition => to.list_style_position = from.list_style_position,
        ListStyleImage => {}
        Width => to.width = from.width,
        Height => to.height = from.height,
        MinWidth => to.min_width = from.min_width,
        MinHeight => to.min_height = from.min_height,
        MaxWidth => to.max_width = from.max_width,
        MaxHeight => to.max_height = from.max_height,
        BoxShadow => to.box_shadow = from.box_shadow.clone(),
        OutlineWidth => to.outline_width = from.outline_width,
        OutlineStyle => to.outline_style = from.outline_style,
        OutlineColor => to.outline_color = from.outline_color,
        OutlineOffset => to.outline_offset = from.outline_offset,
        BackgroundColor => to.background_color = from.background_color,
        BackgroundImage => {
            to.backgrounds_mut().background_image = from.backgrounds.background_image.clone()
        }
        BackgroundPositionX => {
            to.backgrounds_mut().background_position_x =
                from.backgrounds.background_position_x.clone()
        }
        BackgroundPositionY => {
            to.backgrounds_mut().background_position_y =
                from.backgrounds.background_position_y.clone()
        }
        BackgroundSize => {
            to.backgrounds_mut().background_size = from.backgrounds.background_size.clone()
        }
        BackgroundRepeat => {
            to.backgrounds_mut().background_repeat = from.backgrounds.background_repeat.clone()
        }
        BackgroundAttachment => {
            to.backgrounds_mut().background_attachment =
                from.backgrounds.background_attachment.clone()
        }
        BackgroundClip => {
            to.backgrounds_mut().background_clip = from.backgrounds.background_clip.clone()
        }
        BackgroundOrigin => {
            to.backgrounds_mut().background_origin = from.backgrounds.background_origin.clone()
        }
        TextDecorationLine => to.text_decoration_line = from.text_decoration_line,
        TextDecorationStyle => {
            to.rare_mut().text_decoration_style = from.rare.text_decoration_style
        }
        TextDecorationColor => {
            to.rare_mut().text_decoration_color = from.rare.text_decoration_color
        }
        TextDecorationThickness => {
            to.rare_mut().text_decoration_thickness = from.rare.text_decoration_thickness
        }
        TextUnderlineOffset => {
            to.rare_inherited_mut().text_underline_offset =
                from.rare_inherited.text_underline_offset
        }
        TextUnderlinePosition => {
            to.rare_inherited_mut().text_underline_position =
                from.rare_inherited.text_underline_position
        }
        TextShadow => to.rare_inherited_mut().text_shadow = from.rare_inherited.text_shadow.clone(),
        WebkitTextFillColor => {
            to.rare_inherited_mut().text_fill_color = from.rare_inherited.text_fill_color
        }
        Filter => to.rare_mut().filter = from.rare.filter.clone(),
        Isolation => to.rare_mut().isolation = from.rare.isolation,
        Opacity => to.opacity = from.opacity,
        OverflowX => to.overflow_x = from.overflow_x,
        OverflowY => to.overflow_y = from.overflow_y,
        Transform => to.transform = from.transform.clone(),
        TransformOrigin => to.rare_mut().transform_origin = from.rare.transform_origin,
        TransformBox => to.rare_mut().transform_box = from.rare.transform_box,
        ObjectFit => to.rare_mut().object_fit = from.rare.object_fit,
        ObjectPosition => to.rare_mut().object_position = from.rare.object_position,
        WebkitBoxOrient => to.rare_mut().box_orient = from.rare.box_orient,
        WebkitBoxDirection => to.rare_mut().box_direction = from.rare.box_direction,
        WebkitBoxPack => to.rare_mut().box_pack = from.rare.box_pack,
        WebkitBoxAlign => to.rare_mut().box_align = from.rare.box_align,
        WebkitBoxFlex => to.rare_mut().box_flex = from.rare.box_flex,
        WebkitBoxOrdinalGroup => to.rare_mut().box_ordinal_group = from.rare.box_ordinal_group,
        WebkitLineClamp => to.rare_mut().line_clamp = from.rare.line_clamp,
        Contain => to.rare_mut().contain = from.rare.contain,
        Clip => to.rare_mut().clip = from.rare.clip,
        ClipPath => to.rare_mut().clip_path = from.rare.clip_path.clone(),
        FlexDirection => to.flex_direction = from.flex_direction,
        FlexWrap => to.flex_wrap = from.flex_wrap,
        FlexGrow => to.flex_grow = from.flex_grow,
        FlexShrink => to.flex_shrink = from.flex_shrink,
        FlexBasis => to.flex_basis = from.flex_basis,
        Order => to.order = from.order,
        JustifyContent => to.justify_content = from.justify_content,
        AlignContent => to.align_content = from.align_content,
        AlignItems => to.align_items = from.align_items,
        AlignSelf => to.align_self = from.align_self,
        RowGap => to.row_gap = from.row_gap,
        ColumnGap => to.column_gap = from.column_gap,
        ZIndex => to.z_index = from.z_index,
        GridTemplateColumns => {
            to.rare_mut().grid_template_columns = from.rare.grid_template_columns.clone()
        }
        GridTemplateRows => to.rare_mut().grid_template_rows = from.rare.grid_template_rows.clone(),
        GridTemplateAreas => {
            to.rare_mut().grid_template_areas = from.rare.grid_template_areas.clone()
        }
        GridAutoColumns => to.rare_mut().grid_auto_columns = from.rare.grid_auto_columns.clone(),
        GridAutoRows => to.rare_mut().grid_auto_rows = from.rare.grid_auto_rows.clone(),
        GridAutoFlow => to.rare_mut().grid_auto_flow = from.rare.grid_auto_flow,
        GridRowStart => to.rare_mut().grid_row_start = from.rare.grid_row_start.clone(),
        GridRowEnd => to.rare_mut().grid_row_end = from.rare.grid_row_end.clone(),
        GridColumnStart => to.rare_mut().grid_column_start = from.rare.grid_column_start.clone(),
        GridColumnEnd => to.rare_mut().grid_column_end = from.rare.grid_column_end.clone(),
        JustifyItems => to.justify_items = from.justify_items,
        JustifySelf => to.justify_self = from.justify_self,
        Color => to.color = from.color,
        FontFamily => set_font!(to, family, from.font.family.clone()),
        FontSize => set_font!(to, size, from.font.size),
        FontWeight => set_font!(to, weight, from.font.weight),
        FontStyle => set_font!(to, style, from.font.style),
        FontStretch => set_font!(to, stretch, from.font.stretch),
        FontVariantLigatures => set_font!(to, variant_ligatures, from.font.variant_ligatures),
        FontVariantNumeric => set_font!(to, variant_numeric, from.font.variant_numeric),
        FontVariantEastAsian => set_font!(to, variant_east_asian, from.font.variant_east_asian),
        FontVariantCaps => set_font!(to, variant_caps, from.font.variant_caps),
        FontVariantAlternates => {
            set_font!(to, variant_alternates, from.font.variant_alternates.clone())
        }
        FontVariantPosition => set_font!(to, variant_position, from.font.variant_position),
        FontVariantEmoji => set_font!(to, variant_emoji, from.font.variant_emoji),
        FontSizeAdjust => set_font!(to, size_adjust, from.font.size_adjust),
        FontLanguageOverride => {
            set_font!(to, language_override, from.font.language_override.clone())
        }
        FontKerning => set_font!(to, kerning, from.font.kerning),
        FontOpticalSizing => set_font!(to, optical_sizing, from.font.optical_sizing),
        FontFeatureSettings => {
            set_font!(to, feature_settings, from.font.feature_settings.clone())
        }
        FontVariationSettings => {
            set_font!(to, variation_settings, from.font.variation_settings.clone())
        }
        FontSynthesisWeight => set_font!(to, synthesis_weight, from.font.synthesis_weight),
        FontSynthesisStyle => set_font!(to, synthesis_style, from.font.synthesis_style),
        FontSynthesisSmallCaps => {
            set_font!(to, synthesis_small_caps, from.font.synthesis_small_caps)
        }
        LineHeight => to.line_height = from.line_height,
        TextAlign => to.text_align = from.text_align,
        VerticalAlign => to.vertical_align = from.vertical_align,
        WhiteSpaceCollapse => to.white_space_collapse = from.white_space_collapse,
        TextWrapMode => to.text_wrap_mode = from.text_wrap_mode,
        TextTransform => to.text_transform = from.text_transform,
        TextOverflow => to.text_overflow = from.text_overflow,
        Appearance => to.appearance = from.appearance,
        LetterSpacing => {
            to.rare_inherited_mut().letter_spacing = from.rare_inherited.letter_spacing
        }
        WordSpacing => to.rare_inherited_mut().word_spacing = from.rare_inherited.word_spacing,
        TabSize => to.rare_inherited_mut().tab_size = from.rare_inherited.tab_size,
        TextIndent => to.rare_inherited_mut().text_indent = from.rare_inherited.text_indent,
        WordBreak => to.rare_inherited_mut().word_break = from.rare_inherited.word_break,
        OverflowWrap => to.rare_inherited_mut().overflow_wrap = from.rare_inherited.overflow_wrap,
        Fill => to.svg_mut().fill = from.svg.fill.clone(),
        FillOpacity => to.svg_mut().fill_opacity = from.svg.fill_opacity,
        FillRule => to.svg_mut().fill_rule = from.svg.fill_rule,
        ClipRule => to.svg_mut().clip_rule = from.svg.clip_rule,
        TextAnchor => to.svg_mut().text_anchor = from.svg.text_anchor,
        Stroke => to.svg_mut().stroke = from.svg.stroke.clone(),
        StrokeOpacity => to.svg_mut().stroke_opacity = from.svg.stroke_opacity,
        StopColor => to.svg_mut().stop_color = from.svg.stop_color,
        StopOpacity => to.svg_mut().stop_opacity = from.svg.stop_opacity,
        StrokeWidth => to.svg_mut().stroke_width = from.svg.stroke_width,
        StrokeLinecap => to.svg_mut().stroke_linecap = from.svg.stroke_linecap,
        StrokeLinejoin => to.svg_mut().stroke_linejoin = from.svg.stroke_linejoin,
        StrokeMiterlimit => to.svg_mut().stroke_miterlimit = from.svg.stroke_miterlimit,
        StrokeDasharray => to.svg_mut().stroke_dasharray = from.svg.stroke_dasharray.clone(),
        StrokeDashoffset => to.svg_mut().stroke_dashoffset = from.svg.stroke_dashoffset,
        Cx => to.svg_mut().cx = from.svg.cx,
        Cy => to.svg_mut().cy = from.svg.cy,
        R => to.svg_mut().r = from.svg.r,
        Rx => to.svg_mut().rx = from.svg.rx,
        Ry => to.svg_mut().ry = from.svg.ry,
        X => to.svg_mut().x = from.svg.x,
        Y => to.svg_mut().y = from.svg.y,
        D => to.svg_mut().d = from.svg.d.clone(),
        Visibility => to.visibility = from.visibility,
        WebkitTextSecurity => {
            to.rare_inherited_mut().text_security = from.rare_inherited.text_security
        }
        PointerEvents => to.pointer_events = from.pointer_events,
        BorderCollapse => to.border_collapse = from.border_collapse,
        WebkitBorderHorizontalSpacing => {
            to.rare_inherited_mut().border_spacing.horizontal =
                from.rare_inherited.border_spacing.horizontal
        }
        WebkitBorderVerticalSpacing => {
            to.rare_inherited_mut().border_spacing.vertical =
                from.rare_inherited.border_spacing.vertical
        }
        TableLayout => to.table_layout = from.table_layout,
        CaptionSide => to.caption_side = from.caption_side,
        EmptyCells => to.empty_cells = from.empty_cells,
        UserSelect => to.rare_inherited_mut().user_select = from.rare_inherited.user_select,
        WebkitUserModify => to.rare_inherited_mut().user_modify = from.rare_inherited.user_modify,
        Hyphens => to.rare_inherited_mut().hyphens = from.rare_inherited.hyphens,
        ForcedColorAdjust => {
            to.rare_inherited_mut().forced_color_adjust = from.rare_inherited.forced_color_adjust
        }
        WebkitTapHighlightColor => {
            to.rare_inherited_mut().tap_highlight_color = from.rare_inherited.tap_highlight_color
        }
        CaretColor => to.rare_inherited_mut().caret_color = from.rare_inherited.caret_color,
        TextSizeAdjust => {
            to.rare_inherited_mut().text_size_adjust = from.rare_inherited.text_size_adjust
        }
        TextRendering => {
            set_font!(to, text_rendering, from.font.text_rendering)
        }
        WebkitFontSmoothing => {
            set_font!(to, smoothing, from.font.smoothing)
        }
        Resize => to.rare_mut().resize = from.rare.resize,
        ScrollBehavior => to.rare_mut().scroll_behavior = from.rare.scroll_behavior,
        OverflowAnchor => to.rare_mut().overflow_anchor = from.rare.overflow_anchor,
        Cursor => {
            if to.rare_inherited.cursor != from.rare_inherited.cursor {
                to.rare_inherited_mut().cursor = from.rare_inherited.cursor.clone();
            }
        }
        BorderImageSource => {
            to.rare_mut().border_image.source = from.rare.border_image.source.clone()
        }
        BorderImageSlice => to.rare_mut().border_image.slice = from.rare.border_image.slice,
        BorderImageWidth => to.rare_mut().border_image.width = from.rare.border_image.width,
        BorderImageOutset => to.rare_mut().border_image.outset = from.rare.border_image.outset,
        BorderImageRepeat => to.rare_mut().border_image.repeat = from.rare.border_image.repeat,
        TransitionProperty => to.transitions.property = from.transitions.property.clone(),
        TransitionDuration => to.transitions.duration = from.transitions.duration.clone(),
        TransitionTimingFunction => {
            to.transitions.timing_function = from.transitions.timing_function.clone();
        }
        TransitionDelay => to.transitions.delay = from.transitions.delay.clone(),
        TransitionBehavior => to.transitions.behavior = from.transitions.behavior.clone(),
        AnimationDuration => {
            to.rare_mut().animations.duration = from.rare.animations.duration.clone()
        }
        AnimationTimingFunction => {
            to.rare_mut().animations.timing_function = from.rare.animations.timing_function.clone();
        }
        AnimationDelay => to.rare_mut().animations.delay = from.rare.animations.delay.clone(),
        AnimationIterationCount => {
            to.rare_mut().animations.iteration_count = from.rare.animations.iteration_count.clone();
        }
        AnimationDirection => {
            to.rare_mut().animations.direction = from.rare.animations.direction.clone()
        }
        AnimationFillMode => {
            to.rare_mut().animations.fill_mode = from.rare.animations.fill_mode.clone()
        }
        AnimationPlayState => {
            to.rare_mut().animations.play_state = from.rare.animations.play_state.clone()
        }
        AnimationName => to.rare_mut().animations.name = from.rare.animations.name.clone(),
        AnimationTimeline => {
            to.rare_mut().animations.timeline = from.rare.animations.timeline.clone()
        }
        AnimationRangeStart => {
            to.rare_mut().animations.range_start = from.rare.animations.range_start.clone()
        }
        AnimationRangeEnd => {
            to.rare_mut().animations.range_end = from.rare.animations.range_end.clone()
        }
        _ => {
            if let Some(i) = side_of(MARGIN) {
                *side_mut(&mut to.margin, i) = *side(&from.margin, i);
            } else if let Some(i) = side_of(PADDING) {
                *side_mut(&mut to.padding, i) = *side(&from.padding, i);
            } else if let Some(i) = side_of(INSET) {
                *side_mut(&mut to.inset, i) = *side(&from.inset, i);
            } else if let Some(i) = side_of(BORDER_WIDTH) {
                *side_mut(&mut to.border_width, i) = *side(&from.border_width, i);
            } else if let Some(i) = side_of(BORDER_STYLE) {
                *side_mut(&mut to.border_style, i) = *side(&from.border_style, i);
            } else if let Some(i) = side_of(BORDER_COLOR) {
                *side_mut(&mut to.border_color, i) = *side(&from.border_color, i);
            } else if BORDER_RADIUS.contains(&id) {
                let i = corner(id);
                *corner_mut(&mut to.rare_mut().border_radius, i) =
                    *corner_ref(&from.rare.border_radius, i);
            } else {
                unreachable!("{id:?} has no copy rule")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_longhand_at_its_index() {
        for (i, id) in LonghandId::ALL.iter().enumerate() {
            assert_eq!(id.index(), i, "{id:?}");
        }
        // The last variant is the last one listed.
        assert_eq!(MaxBlockSize.index(), LonghandId::COUNT - 1);
    }
}
