//! Serialization as Chrome writes it (CSSOM's "serialize a CSS value";
//! Blink's `CSSValue::CssText` and `StylePropertySerializer`): a
//! longhand's declared value, and a shorthand made back from its
//! longhands. Each property added to the supported set adds its value
//! here.

use computed_style::{
    BackgroundAttachment, BackgroundBox, BackgroundRepeat, BorderStyle, BoxSizing, Clear, Content,
    ContentAlignment, ContentItem, CounterChange, CounterReset, Display, FlexDirection, FlexWrap,
    Float, FontStyle, GridLine, ItemAlignment, ListStylePosition, ListStyleType, OverflowWrap,
    Position, StrokeLinecap, StrokeLinejoin, TableDisplay, TextAlign, TextDecorationLine,
    TextOverflow, TextTransform, TextWrapMode, VerticalAlign, Visibility, WhiteSpace,
    WhiteSpaceCollapse, WordBreak,
};

use crate::animation::{
    animation_duration_to_css, animation_name_to_css, animation_to_css, fill_mode_name,
    iteration_count_to_css, play_state_name, playback_direction_name, transition_behavior_name,
    transition_property_to_css, transition_to_css,
};
use crate::background::{
    SpecifiedBackgroundSize, SpecifiedImage, SpecifiedPositionComponent, attachments_to_css,
    box_name, boxes_to_css, repeats_to_css, specified_list_to_css,
};
use crate::effects::{transform_box_name, transform_origin_to_css, transform_to_css};
use crate::font::{FontWeight, LineHeightValue, font_family_to_css};
use crate::grid::{
    auto_flow_to_css, grid_line_to_css, template_area_rows, template_areas_to_css,
    track_list_to_css, track_sizes_to_css,
};
use crate::properties::{CssWideKeyword, LonghandId, OverflowValue, Shorthand, SizeValue, Value};
use crate::shadow::shadows_to_css;
use crate::svg::{d_to_css, dasharray_to_css};
use crate::values::{SpecifiedColor, write_number};

/// A longhand's declared value.
pub(crate) fn value_to_css(value: &Value, dest: &mut String) {
    let keyword = |dest: &mut String, k: &str| dest.push_str(k);
    match value {
        Value::Display(v) => keyword(dest, display_name(*v)),
        Value::Position(v) => keyword(dest, position_name(*v)),
        Value::Float(v) => keyword(dest, float_name(*v)),
        Value::Clear(v) => keyword(dest, clear_name(*v)),
        Value::Content(v) => content(v, dest),
        Value::CounterReset(v) => counter_reset(v, dest),
        Value::CounterChanges(v) => counter_changes(v, dest),
        Value::ListStyleType(v) => list_style_type(v, dest),
        Value::ListStylePosition(v) => keyword(dest, list_style_position_name(*v)),
        Value::ListStyleImage => keyword(dest, "none"),
        Value::BoxSizing(v) => keyword(dest, box_sizing_name(*v)),
        Value::AutoOr(None) => keyword(dest, "auto"),
        Value::AutoOr(Some(v)) | Value::NoneOr(Some(v)) | Value::LengthPercentage(v) => {
            v.to_css(dest)
        }
        Value::NoneOr(None) => keyword(dest, "none"),
        Value::Size(v) => size_to_css(v, dest),
        Value::LineWidth(v) => v.to_css(dest),
        Value::BorderStyle(v) => keyword(dest, border_style_name(*v)),
        Value::OutlineStyle(v) => keyword(dest, outline_style_name(*v)),
        Value::Length(v) => v.to_css(dest),
        Value::BoxShadows(v) => shadows_to_css(v, dest),
        Value::Color(v) => v.to_css(dest),
        Value::CaretColor(None) => keyword(dest, "auto"),
        Value::CaretColor(Some(v)) => v.to_css(dest),
        Value::CornerRadius(x, y) => {
            x.to_css(dest);
            if let Some(y) = y.as_ref().filter(|y| *y != x) {
                dest.push(' ');
                y.to_css(dest);
            }
        }
        Value::BackgroundImages(v) => {
            specified_list_to_css(v, dest, |image, dest| image.to_css(dest))
        }
        Value::PositionsX(v) => specified_list_to_css(v, dest, |c, dest| c.to_css(0, dest)),
        Value::PositionsY(v) => specified_list_to_css(v, dest, |c, dest| c.to_css(1, dest)),
        Value::BackgroundSizes(v) => specified_list_to_css(v, dest, |size, dest| size.to_css(dest)),
        Value::BackgroundRepeats(v) => repeats_to_css(v, dest),
        Value::BackgroundAttachments(v) => attachments_to_css(v, dest),
        Value::BackgroundBoxes(v) => boxes_to_css(v, dest),
        Value::TextDecorationLine(v) => text_decoration_line_to_css(*v, dest),
        Value::TextDecorationStyle(v) => {
            keyword(dest, crate::text_decor::text_decoration_style_name(*v))
        }
        Value::TextDecorationThickness(v) => v.to_css(dest),
        Value::TextUnderlinePosition(v) => crate::text_decor::underline_position_to_css(*v, dest),
        Value::Filter(v) => crate::filter::filter_to_css(v, dest),
        Value::Isolation(v) => keyword(dest, crate::filter::isolation_name(*v)),
        Value::Opacity(v) | Value::Number(v) => write_number(dest, (*v).into()),
        Value::Overflow(v) => keyword(dest, overflow_name(*v)),
        Value::Gap(None) => keyword(dest, "normal"),
        Value::Gap(Some(v)) => v.to_css(dest),
        Value::Transform(v) => transform_to_css(v, dest),
        Value::TransformOrigin(v) => transform_origin_to_css(v, dest),
        Value::TabSize(crate::properties::SpecifiedTabSize::Spaces(v)) => {
            write_number(dest, (*v).into())
        }
        Value::TabSize(crate::properties::SpecifiedTabSize::Length(v)) => v.to_css(dest),
        Value::TransformBox(v) => keyword(dest, transform_box_name(*v)),
        Value::ObjectFit(v) => keyword(dest, object_fit_name(*v)),
        Value::ObjectPosition(v) => v.to_css(dest),
        Value::BoxOrient(v) => keyword(dest, box_orient_name(*v)),
        Value::BoxDirection(v) => keyword(dest, box_direction_name(*v)),
        Value::BoxPack(v) => keyword(dest, box_pack_name(*v)),
        Value::BoxAlign(v) => keyword(dest, box_align_name(*v)),
        Value::Clip(v) => crate::masking::specified_clip_to_css(v, dest),
        Value::ClipPath(v) => crate::masking::specified_clip_path_to_css(v, dest),
        Value::Contain(crate::properties::SpecifiedContain::Strict) => keyword(dest, "strict"),
        Value::Contain(crate::properties::SpecifiedContain::Content) => keyword(dest, "content"),
        Value::Contain(crate::properties::SpecifiedContain::Each(v)) => {
            crate::properties::contain_to_css(*v, false, dest)
        }
        Value::LineClamp(None) => keyword(dest, "none"),
        Value::LineClamp(Some(v)) => dest.push_str(&v.to_string()),
        Value::FlexDirection(v) => keyword(dest, flex_direction_name(*v)),
        Value::FlexWrap(v) => keyword(dest, flex_wrap_name(*v)),
        Value::Integer(v) => dest.push_str(&v.to_string()),
        Value::AutoOrInteger(None) => keyword(dest, "auto"),
        Value::AutoOrInteger(Some(v)) => dest.push_str(&v.to_string()),
        Value::ContentAlignment(v) => keyword(dest, content_alignment_name(*v)),
        Value::ItemAlignment(v) => keyword(dest, item_alignment_name(*v)),
        Value::AlignSelf(None) => keyword(dest, "auto"),
        Value::AlignSelf(Some(v)) => keyword(dest, item_alignment_name(*v)),
        Value::TrackList(v) => track_list_to_css(v, dest),
        Value::Areas(v) => template_areas_to_css(v.as_deref(), dest),
        Value::TrackSizes(v) => track_sizes_to_css(v, dest),
        Value::GridAutoFlow(v) => auto_flow_to_css(*v, dest),
        Value::GridLine(v) => grid_line_to_css(v, dest),
        Value::FontFamily(v) => font_family_to_css(v, dest),
        Value::FontSize(v) => v.to_css(dest),
        Value::FontWeight(v) => v.to_css(dest),
        Value::FontStyle(v) => keyword(dest, font_style_name(*v)),
        Value::FontStretch(v) => crate::font_features::stretch_to_css(*v, dest),
        Value::FontVariantLigatures(v) => crate::font_features::ligatures_to_css(*v, dest),
        Value::FontVariantNumeric(v) => crate::font_features::numeric_to_css(*v, dest),
        Value::FontVariantEastAsian(v) => crate::font_features::east_asian_to_css(*v, dest),
        Value::FontVariantCaps(v) => keyword(dest, crate::font_features::caps_name(*v)),
        Value::FontVariantAlternates(v) => crate::font_features::alternates_to_css(v, dest),
        Value::FontVariantPosition(v) => keyword(dest, crate::font_features::position_name(*v)),
        Value::FontVariantEmoji(v) => keyword(dest, crate::font_features::emoji_name(*v)),
        Value::FontSizeAdjust(v) => crate::font_features::size_adjust_to_css(*v, dest),
        Value::FontLanguageOverride(v) => {
            crate::font_features::language_override_to_css(v.as_deref(), dest)
        }
        Value::FontKerning(v) => keyword(dest, crate::font_features::kerning_name(*v)),
        Value::FontOpticalSizing(v) => keyword(dest, crate::font_features::auto_or_none(*v)),
        Value::FontFeatureSettings(v) => crate::font_features::feature_settings_to_css(v, dest),
        Value::FontVariationSettings(v) => crate::font_features::variation_settings_to_css(v, dest),
        Value::FontSynthesis(v) => keyword(dest, crate::font_features::auto_or_none(*v)),
        Value::LineHeight(v) => v.to_css(dest),
        Value::TextAlign(v) => keyword(dest, text_align_name(*v)),
        Value::VerticalAlign(v) => keyword(dest, vertical_align_name(*v)),
        Value::WhiteSpaceCollapse(v) => keyword(dest, white_space_collapse_name(*v)),
        Value::TextWrapMode(v) => keyword(dest, text_wrap_mode_name(*v)),
        Value::TextTransform(v) => keyword(dest, text_transform_name(*v)),
        Value::TextOverflow(v) => keyword(dest, text_overflow_name(*v)),
        Value::Appearance(v) => keyword(dest, appearance_name(*v)),
        // A longhand alone reads back as nothing (Chrome's).
        Value::SystemFont(_) => {}
        Value::Spacing(None) => keyword(dest, "normal"),
        Value::Spacing(Some(v)) => v.to_css(dest),
        Value::WordBreak(v) => keyword(dest, word_break_name(*v)),
        Value::OverflowWrap(v) => keyword(dest, overflow_wrap_name(*v)),
        Value::Paint(v) => v.to_css(dest),
        Value::FillRule(v) => keyword(dest, fill_rule_name(*v)),
        Value::TextAnchor(v) => keyword(dest, text_anchor_name(*v)),
        Value::StrokeLinecap(v) => keyword(dest, stroke_linecap_name(*v)),
        Value::StrokeLinejoin(v) => keyword(dest, stroke_linejoin_name(*v)),
        Value::Dasharray(v) => dasharray_to_css(v.as_deref(), dest),
        Value::Path(v) => d_to_css(v.as_deref(), dest),
        Value::Visibility(v) => keyword(dest, visibility_name(*v)),
        Value::TextSecurity(v) => keyword(dest, text_security_name(*v)),
        Value::PointerEvents(v) => keyword(dest, pointer_events_name(*v)),
        Value::BorderCollapse(v) => keyword(dest, border_collapse_name(*v)),
        Value::TableLayout(v) => keyword(dest, table_layout_name(*v)),
        Value::CaptionSide(v) => keyword(dest, caption_side_name(*v)),
        Value::EmptyCells(v) => keyword(dest, empty_cells_name(*v)),
        Value::UserSelect(v) => keyword(dest, user_select_name(*v)),
        Value::UserModify(v) => keyword(dest, user_modify_name(*v)),
        Value::Hyphens(v) => keyword(dest, hyphens_name(*v)),
        Value::ForcedColorAdjust(v) => keyword(dest, forced_color_adjust_name(*v)),
        Value::TextSizeAdjust(v) => match v {
            crate::properties::SpecifiedTextSizeAdjust::Auto => keyword(dest, "auto"),
            crate::properties::SpecifiedTextSizeAdjust::None => keyword(dest, "none"),
            crate::properties::SpecifiedTextSizeAdjust::Percent(percent) => {
                percent_to_css(*percent, dest)
            }
        },
        Value::TextRendering(v) => keyword(dest, text_rendering_name(*v)),
        Value::FontSmoothing(v) => keyword(dest, font_smoothing_name(*v)),
        Value::Resize(v) => keyword(dest, resize_name(*v)),
        Value::ScrollBehavior(v) => keyword(dest, scroll_behavior_name(*v)),
        Value::OverflowAnchor(v) => keyword(dest, overflow_anchor_name(*v)),
        Value::Cursor(v) => v.to_css(dest),
        Value::BorderImageSource(v) => v.to_css(dest),
        Value::BorderImageSlice(v) => crate::border_image::slice_to_css(v, dest),
        Value::BorderImageWidth(v) => crate::border_image::specified_width_to_css(v, dest),
        Value::BorderImageOutset(v) => crate::border_image::specified_outset_to_css(v, dest),
        Value::BorderImageRepeat(horizontal, vertical) => {
            crate::border_image::repeat_to_css(*horizontal, *vertical, dest)
        }
        Value::TransitionProperties(v) => {
            specified_list_to_css(v, dest, transition_property_to_css)
        }
        Value::Times(v) => specified_list_to_css(v, dest, |time, dest| time.to_css(dest)),
        Value::Easings(v) => specified_list_to_css(v, dest, |easing, dest| easing.to_css(dest)),
        Value::TransitionBehaviors(v) => specified_list_to_css(v, dest, |behavior, dest| {
            dest.push_str(transition_behavior_name(*behavior))
        }),
        Value::AnimationDurations(v) => specified_list_to_css(v, dest, animation_duration_to_css),
        Value::IterationCounts(v) => {
            specified_list_to_css(v, dest, |count, dest| iteration_count_to_css(*count, dest))
        }
        Value::PlaybackDirections(v) => specified_list_to_css(v, dest, |direction, dest| {
            dest.push_str(playback_direction_name(*direction))
        }),
        Value::FillModes(v) => {
            specified_list_to_css(v, dest, |fill, dest| dest.push_str(fill_mode_name(*fill)))
        }
        Value::PlayStates(v) => specified_list_to_css(v, dest, |state, dest| {
            dest.push_str(play_state_name(*state))
        }),
        Value::AnimationNames(v) => specified_list_to_css(v, dest, animation_name_to_css),
        Value::AnimationTimelines(v) => {
            specified_list_to_css(v, dest, crate::animation::animation_timeline_to_css)
        }
        Value::AnimationRangeBoundaries(v) => {
            specified_list_to_css(v, dest, crate::animation::animation_range_boundary_to_css)
        }
        Value::CssWide(v) => keyword(dest, css_wide_name(*v)),
        // As written; a shorthand's longhand reads as the empty string
        // (only the shorthand has a value), as in Chrome.
        Value::Unparsed(v) => {
            if v.shorthand.is_none() {
                dest.push_str(&v.css);
            }
        }
    }
}

pub(crate) fn value_text(value: &Value) -> String {
    let mut text = String::new();
    value_to_css(value, &mut text);
    text
}

/// A shorthand made back from its longhands' values (in
/// `Shorthand::longhands` order, each with whether the shorthand left it
/// out), none of them a CSS-wide keyword; `None` if this shorthand can't
/// express them.
pub(crate) fn shorthand_to_css(shorthand: Shorthand, values: &[(&Value, bool)]) -> Option<String> {
    let text = |i: usize| value_text(values[i].0);
    let same = |i: usize, j: usize| values[i] == values[j];
    Some(match shorthand {
        Shorthand::Sides(_) => sides_to_css(&[text(0), text(1), text(2), text(3)]),
        Shorthand::BorderSide(_) | Shorthand::FlowBorderSide(_) => border_side_to_css(values),
        // One value for both sides.
        Shorthand::BorderAxis(_) => {
            if !(same(0, 1) && same(2, 3) && same(4, 5)) {
                return None;
            }
            border_side_to_css(&[values[0], values[2], values[4]])
        }
        Shorthand::Border => {
            // One value for every side, and `border-image` as `border`
            // sets it: initial.
            // Each side's width, style and color as the top's.
            for (width, style, color) in [(1, 5, 9), (2, 6, 10), (3, 7, 11)] {
                if !(same(0, width) && same(4, style) && same(8, color)) {
                    return None;
                }
            }
            if values[12..]
                .iter()
                .zip(crate::border_image::initial_values())
                .any(|((value, _), initial)| **value != initial)
            {
                return None;
            }
            border_side_to_css(&[values[0], values[4], values[8]])
        }
        Shorthand::BorderImage => crate::border_image::border_image_to_css(values),
        // One length when they're the same (Chrome's).
        Shorthand::BorderSpacing => {
            if same(0, 1) {
                text(0)
            } else {
                format!("{} {}", text(0), text(1))
            }
        }
        Shorthand::BorderRadius => border_radius_to_css(values),
        // Color, style, width, as `border-top` is written.
        Shorthand::Outline => border_side_to_css(values),
        Shorthand::Background => background_to_css(values)?,
        Shorthand::BackgroundPosition => {
            let (Value::PositionsX(x), Value::PositionsY(y)) = (values[0].0, values[1].0) else {
                unreachable!("background-position's longhands")
            };
            if x.len() != y.len() {
                return None;
            }
            let layers: Vec<String> = x
                .iter()
                .zip(y.iter())
                .map(|(x, y)| position_to_css(x, y))
                .collect();
            layers.join(", ")
        }
        // The values that aren't initial (Blink's, the same for the
        // computed value); `none` if none of them.
        Shorthand::TextDecoration => {
            let initial = [
                matches!(values[0].0, Value::TextDecorationLine(line) if line.is_none()),
                matches!(
                    values[1].0,
                    Value::TextDecorationThickness(crate::text_decor::SpecifiedThickness::Auto)
                ),
                matches!(
                    values[2].0,
                    Value::TextDecorationStyle(computed_style::TextDecorationStyle::Solid)
                ),
                matches!(values[3].0, Value::Color(SpecifiedColor::CurrentColor)),
            ];
            let parts: Vec<String> = (0..4).filter(|&i| !initial[i]).map(text).collect();
            if parts.is_empty() {
                "none".to_owned()
            } else {
                parts.join(" ")
            }
        }
        Shorthand::Font => font_to_css(values)?,
        Shorthand::FontVariant => crate::font_features::font_variant_to_css(values)?,
        Shorthand::FontSynthesis => {
            let mut set = [false; 3];
            for (on, (value, _)) in set.iter_mut().zip(values) {
                let Value::FontSynthesis(value) = value else {
                    unreachable!("font-synthesis's longhands")
                };
                *on = *value;
            }
            let mut text = String::new();
            crate::font_features::synthesis_to_css(set, &mut text);
            text
        }
        Shorthand::Overflow | Shorthand::Gap | Shorthand::Place(_) | Shorthand::Pair(_) => {
            if same(0, 1) {
                text(0)
            } else {
                format!("{} {}", text(0), text(1))
            }
        }
        Shorthand::Flex => format!("{} {} {}", text(0), text(1), text(2)),
        Shorthand::FlexFlow => {
            let row = matches!(values[0].0, Value::FlexDirection(FlexDirection::Row));
            match values[1].0 {
                Value::FlexWrap(FlexWrap::Nowrap) => text(0),
                _ if row => text(1),
                _ => format!("{} {}", text(0), text(1)),
            }
        }
        // The values that aren't initial, as Chrome writes them: a lone
        // `none` for both the image and the type; `outside` if they all
        // are, or if the type is an `inside` or `outside` counter style
        // (to be read back as one).
        Shorthand::ListStyle => {
            let outside = matches!(
                values[0].0,
                Value::ListStylePosition(ListStylePosition::Outside)
            );
            let kind = text(2);
            let mut parts = Vec::new();
            if !outside || kind == "inside" || kind == "outside" {
                parts.push(text(0));
            }
            match values[2].0 {
                Value::ListStyleType(ListStyleType::Disc) => {}
                _ => parts.push(kind),
            }
            if parts.is_empty() {
                parts.push(text(0));
            }
            parts.join(" ")
        }
        Shorthand::WhiteSpace => {
            let (Value::WhiteSpaceCollapse(collapse), Value::TextWrapMode(wrap)) =
                (values[0].0, values[1].0)
            else {
                unreachable!("white-space's longhands")
            };
            match WhiteSpace::from_longhands(*collapse, *wrap) {
                Some(keyword) => white_space_name(keyword).to_owned(),
                None => format!("{} {}", text(0), text(1)),
            }
        }
        Shorthand::TextWrap => text(0),
        Shorthand::GridLines(_) => grid_lines_to_css(&grid_lines(values)?),
        Shorthand::GridArea => {
            // Row start, column start, row end, column end.
            let lines = grid_lines(values)?;
            let mut count = 4;
            if lines[3] == omitted_end(lines[1]) {
                count = 3;
                if lines[2] == omitted_end(lines[0]) {
                    count = 2;
                    if lines[1] == omitted_end(lines[0]) {
                        count = 1;
                    }
                }
            }
            grid_lines_to_css(&lines[..count])
        }
        Shorthand::GridTemplate => grid_template_to_css(values)?,
        Shorthand::Transition => transition_to_css(values)?,
        Shorthand::Animation => animation_to_css(values)?,
        Shorthand::AnimationRange => crate::animation::animation_range_to_css(values)?,
    })
}

/// One layer's position, x then y.
fn position_to_css(x: &SpecifiedPositionComponent, y: &SpecifiedPositionComponent) -> String {
    let mut text = String::new();
    x.to_css(0, &mut text);
    text.push(' ');
    y.to_css(1, &mut text);
    text
}

/// The background longhands' lists (in `BACKGROUND` order).
struct BackgroundLists<'a> {
    images: &'a [SpecifiedImage],
    x: &'a [SpecifiedPositionComponent],
    y: &'a [SpecifiedPositionComponent],
    sizes: &'a [SpecifiedBackgroundSize],
    repeats: &'a [(BackgroundRepeat, BackgroundRepeat)],
    attachments: &'a [BackgroundAttachment],
    origins: &'a [BackgroundBox],
    clips: &'a [BackgroundBox],
    color: &'a Value,
}

impl<'a> BackgroundLists<'a> {
    /// `None` unless every list has a value per layer (Blink's
    /// `GetLayeredShorthandValue` gives up then too).
    fn new(values: &[(&'a Value, bool)]) -> Option<Self> {
        let [
            (Value::BackgroundImages(images), _),
            (Value::PositionsX(x), _),
            (Value::PositionsY(y), _),
            (Value::BackgroundSizes(sizes), _),
            (Value::BackgroundRepeats(repeats), _),
            (Value::BackgroundAttachments(attachments), _),
            (Value::BackgroundBoxes(origins), _),
            (Value::BackgroundBoxes(clips), _),
            (color, _),
        ] = values
        else {
            unreachable!("background's longhands")
        };
        let n = images.len();
        let lengths = [
            x.len(),
            y.len(),
            sizes.len(),
            repeats.len(),
            attachments.len(),
            origins.len(),
            clips.len(),
        ];
        if lengths.iter().any(|&len| len != n) {
            return None;
        }
        Some(Self {
            images,
            x,
            y,
            sizes,
            repeats,
            attachments,
            origins,
            clips,
            color,
        })
    }
}

/// `background` as declared (Blink's `GetLayeredShorthandValue`): each
/// layer's values but those at their initial value, whether the shorthand
/// left them out or not (`padding-box border-box` is `none`), the color
/// last in the last layer; `none` for a layer with nothing to write. A
/// position stays with a size after it.
fn background_to_css(values: &[(&Value, bool)]) -> Option<String> {
    use BackgroundRepeat::Repeat;
    let lists = BackgroundLists::new(values)?;
    let n = lists.images.len();
    let start = SpecifiedPositionComponent::Offset(crate::values::LengthPercentage::Percent(0.0));
    let mut layers = Vec::new();
    for i in 0..n {
        let written = |_: usize, initial: bool| !initial;
        let mut parts = Vec::new();
        let image = &lists.images[i];
        if written(0, *image == SpecifiedImage::None) {
            let mut text = String::new();
            image.to_css(&mut text);
            parts.push(text);
        }
        let size = &lists.sizes[i];
        let position_written = written(1, lists.x[i] == start) || written(2, lists.y[i] == start);
        let size_written = written(3, *size == SpecifiedBackgroundSize::AUTO);
        if position_written || size_written {
            let mut text = position_to_css(&lists.x[i], &lists.y[i]);
            if size_written {
                text.push_str(" / ");
                size.to_css(&mut text);
            }
            parts.push(text);
        }
        if written(4, lists.repeats[i] == (Repeat, Repeat)) {
            let mut text = String::new();
            repeats_to_css(&lists.repeats[i..=i], &mut text);
            parts.push(text);
        }
        if written(5, lists.attachments[i] == BackgroundAttachment::Scroll) {
            let mut text = String::new();
            attachments_to_css(&lists.attachments[i..=i], &mut text);
            parts.push(text);
        }
        let (origin, clip) = (lists.origins[i], lists.clips[i]);
        let origin_written = written(6, origin == BackgroundBox::PaddingBox);
        let clip_written = written(7, clip == BackgroundBox::BorderBox);
        if origin_written || clip_written {
            parts.push(box_name(origin).to_owned());
            // One box stands for both.
            if clip != origin {
                parts.push(box_name(clip).to_owned());
            }
        }
        let transparent = matches!(
            lists.color,
            Value::Color(SpecifiedColor::Named(_, color) | SpecifiedColor::Rgba(color))
                if *color == gfx::Color::TRANSPARENT
        );
        if Some(i) == n.checked_sub(1) && !transparent {
            parts.push(value_text(lists.color));
        }
        if parts.is_empty() {
            parts.push("none".to_owned());
        }
        layers.push(parts.join(" "));
    }
    Some(layers.join(", "))
}

/// `background`'s resolved value as Chrome writes it
/// (`ComputedStyleUtils::ValuesForBackgroundShorthand`): every value of
/// every layer, the color first in the last one.
pub(crate) fn background_computed_to_css(values: &[(&Value, bool)]) -> Option<String> {
    let lists = BackgroundLists::new(values)?;
    let n = lists.images.len();
    let mut layers = Vec::new();
    for i in 0..n {
        let mut text = String::new();
        if Some(i) == n.checked_sub(1) {
            text.push_str(&value_text(lists.color));
            text.push(' ');
        }
        lists.images[i].to_css(&mut text);
        text.push(' ');
        repeats_to_css(&lists.repeats[i..=i], &mut text);
        text.push(' ');
        attachments_to_css(&lists.attachments[i..=i], &mut text);
        text.push(' ');
        text.push_str(&position_to_css(&lists.x[i], &lists.y[i]));
        text.push_str(" / ");
        lists.sizes[i].to_css(&mut text);
        text.push(' ');
        text.push_str(box_name(lists.origins[i]));
        text.push(' ');
        text.push_str(box_name(lists.clips[i]));
        layers.push(text);
    }
    Some(layers.join(", "))
}

/// `border-radius`: the horizontal radii, then `/` and the vertical ones
/// unless they are the same, each list shortened as `margin`'s sides are.
fn border_radius_to_css(values: &[(&Value, bool)]) -> String {
    let mut xs: [String; 4] = Default::default();
    let mut ys: [String; 4] = Default::default();
    for (i, (value, _)) in values.iter().enumerate() {
        let Value::CornerRadius(x, y) = value else {
            unreachable!("border-radius's longhands")
        };
        let mut text = String::new();
        x.to_css(&mut text);
        xs[i] = text;
        let mut text = String::new();
        y.as_ref().unwrap_or(x).to_css(&mut text);
        ys[i] = text;
    }
    let (horizontal, vertical) = (sides_to_css(&xs), sides_to_css(&ys));
    if xs == ys {
        horizontal
    } else {
        format!("{horizontal} / {vertical}")
    }
}

/// `margin` and the like: top, right, bottom, left, with each value that
/// the one opposite it repeats left out.
pub(crate) fn sides_to_css(sides: &[String; 4]) -> String {
    let [top, right, bottom, left] = sides;
    let count = if left != right {
        4
    } else if bottom != top {
        3
    } else if right != top {
        2
    } else {
        1
    };
    sides[..count].join(" ")
}

/// `border-top` and the like: width, style and color, without the ones the
/// shorthand left out (Blink's `GetShorthandValue` skipping implicit
/// initial values).
fn border_side_to_css(values: &[(&Value, bool)]) -> String {
    let parts: Vec<String> = values
        .iter()
        .filter(|(_, implicit)| !implicit)
        .map(|(value, _)| value_text(value))
        .collect();
    parts.join(" ")
}

/// `[ <font-style> ] [ <font-weight> ] <font-size> [ / <line-height> ]
/// <font-family>`, the optional parts left out when `normal` (Blink's
/// `FontValue`).
fn font_to_css(values: &[(&Value, bool)]) -> Option<String> {
    let [
        (style, _),
        (caps, _),
        ..,
        (weight, _),
        (stretch, _),
        (size, _),
        (line_height, _),
        (family, _),
    ] = values
    else {
        unreachable!("font's longhands")
    };
    // A system font: all of them pending on the same one, or the shorthand
    // doesn't read back (one set over them, even to the value it gives, as
    // in Chrome).
    if let Some(system) = values.iter().find_map(|(value, _)| match value {
        Value::SystemFont(system) => Some(*system),
        _ => None,
    }) {
        return values
            .iter()
            .all(|(value, _)| **value == Value::SystemFont(system))
            .then(|| system.name().to_owned());
    }
    // What `font` can't write but as it resets them: `font-variant`'s
    // longhands (but `small-caps`) and the others it has no syntax for.
    let resets_alike = crate::properties::FONT
        .iter()
        .zip(values)
        .filter(|(id, _)| **id != LonghandId::FontVariantCaps)
        .all(|(&id, (value, _))| {
            crate::font_features::font_reset_value(id).is_none_or(|reset| **value == reset)
        });
    if !resets_alike {
        return None;
    }
    let mut parts = Vec::new();
    if !matches!(style, Value::FontStyle(FontStyle::Normal)) {
        parts.push(value_text(style));
    }
    match caps {
        Value::FontVariantCaps(computed_style::FontVariantCaps::Normal) => {}
        Value::FontVariantCaps(computed_style::FontVariantCaps::SmallCaps) => {
            parts.push("small-caps".to_owned())
        }
        _ => return None,
    }
    if !matches!(weight, Value::FontWeight(FontWeight::Normal)) {
        parts.push(value_text(weight));
    }
    // Only a keyword: a percentage is written as the one it is.
    let Value::FontStretch(stretch) = stretch else {
        return None;
    };
    match crate::font_features::stretch_keyword(stretch.percentage())? {
        computed_style::FontStretch::Normal => {}
        keyword => parts.push(crate::font_features::stretch_name(keyword).to_owned()),
    }
    parts.push(value_text(size));
    if !matches!(line_height, Value::LineHeight(LineHeightValue::Normal)) {
        parts.push("/".to_owned());
        parts.push(value_text(line_height));
    }
    parts.push(value_text(family));
    Some(parts.join(" "))
}

fn grid_lines<'a>(values: &[(&'a Value, bool)]) -> Option<Vec<&'a GridLine>> {
    values
        .iter()
        .map(|(value, _)| match value {
            Value::GridLine(line) => Some(line),
            _ => None,
        })
        .collect()
}

/// What a `grid-row` / `grid-column` / `grid-area` line is when omitted
/// after `start`: the same name if `start` is only one, `auto` otherwise.
fn omitted_end(start: &GridLine) -> &GridLine {
    match start {
        GridLine::Ident(_) => start,
        _ => &GridLine::Auto,
    }
}

/// `grid-row` / `grid-column`: start, then the end unless it is what
/// leaving it out gives; `grid-area`: the lines given.
fn grid_lines_to_css(lines: &[&GridLine]) -> String {
    let lines = if let [start, end] = lines
        && *end == omitted_end(start)
    {
        &lines[..1]
    } else {
        lines
    };
    let parts: Vec<String> = lines
        .iter()
        .map(|line| {
            let mut text = String::new();
            grid_line_to_css(line, &mut text);
            text
        })
        .collect();
    parts.join(" / ")
}

/// `grid-template`: rows `/` columns without areas; with areas, each row's
/// line names, string and size (left out if `auto`), then `/` and the
/// columns unless `none`. `None` if the rows can't go with the strings
/// (`repeat()`, or not one per row).
fn grid_template_to_css(values: &[(&Value, bool)]) -> Option<String> {
    use computed_style::{GridTrack, TrackBreadth, TrackSize};

    let [
        (Value::TrackList(rows), _),
        (Value::TrackList(columns), _),
        (Value::Areas(areas), _),
    ] = values
    else {
        unreachable!("grid-template's longhands")
    };
    let mut columns_text = String::new();
    track_list_to_css(columns, &mut columns_text);
    let Some(areas) = areas else {
        if rows.tracks.is_empty() && columns.tracks.is_empty() {
            return Some("none".to_owned());
        }
        let mut text = String::new();
        track_list_to_css(rows, &mut text);
        return Some(format!("{text} / {columns_text}"));
    };
    let strings = template_area_rows(areas);
    if rows.tracks.len() != strings.len() {
        return None;
    }
    let mut parts = Vec::new();
    let names = |i: usize, parts: &mut Vec<String>| {
        if let Some(names) = rows.line_names.get(i).filter(|names| !names.is_empty()) {
            let names: Vec<String> = names
                .iter()
                .map(|name| {
                    let mut text = String::new();
                    cssparser::serialize_identifier(name, &mut text).expect("writing to a String");
                    text
                })
                .collect();
            parts.push(format!("[{}]", names.join(" ")));
        }
    };
    for (i, (track, string)) in rows.tracks.iter().zip(&strings).enumerate() {
        let GridTrack::Size(size) = track else {
            return None;
        };
        names(i, &mut parts);
        let mut text = String::new();
        cssparser::serialize_string(string, &mut text).expect("writing to a String");
        parts.push(text);
        if *size != TrackSize::Breadth(TrackBreadth::Auto) {
            let mut text = String::new();
            track_sizes_to_css(std::slice::from_ref(size), &mut text);
            parts.push(text);
        }
    }
    names(rows.tracks.len(), &mut parts);
    let mut text = parts.join(" ");
    if !columns.tracks.is_empty() {
        text.push_str(" / ");
        text.push_str(&columns_text);
    }
    Some(text)
}

fn size_to_css(size: &SizeValue, dest: &mut String) {
    match size {
        SizeValue::Auto => dest.push_str("auto"),
        SizeValue::LengthPercentage(v) => v.to_css(dest),
        SizeValue::MinContent => dest.push_str("min-content"),
        SizeValue::MaxContent => dest.push_str("max-content"),
        SizeValue::FitContent => dest.push_str("fit-content"),
        SizeValue::Content => dest.push_str("content"),
    }
}

/// `none`, or the lines in the order the grammar lists them.
pub(crate) fn text_decoration_line_to_css(line: TextDecorationLine, dest: &mut String) {
    let lines = [
        (line.underline, "underline"),
        (line.overline, "overline"),
        (line.line_through, "line-through"),
        (line.blink, "blink"),
    ];
    let names: Vec<&str> = lines
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, name)| *name)
        .collect();
    if names.is_empty() {
        dest.push_str("none");
    } else {
        dest.push_str(&names.join(" "));
    }
}

pub(crate) fn css_wide_name(keyword: CssWideKeyword) -> &'static str {
    match keyword {
        CssWideKeyword::Initial => "initial",
        CssWideKeyword::Inherit => "inherit",
        CssWideKeyword::Unset => "unset",
    }
}

pub(crate) fn float_name(float: Float) -> &'static str {
    match float {
        Float::None => "none",
        Float::Left => "left",
        Float::Right => "right",
    }
}

/// `content`, as Chrome writes it: its keyword, or its items with a
/// space between them, each string quoted and each `attr()` as the function
/// (not the value it stands for).
pub(crate) fn content(content: &Content, dest: &mut String) {
    match content {
        Content::Normal => dest.push_str("normal"),
        Content::None => dest.push_str("none"),
        Content::Items(items) => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    dest.push(' ');
                }
                match item {
                    ContentItem::String(string) => {
                        cssparser::serialize_string(string, dest).expect("writing to a String");
                    }
                    ContentItem::Attr { name, .. } => {
                        dest.push_str("attr(");
                        cssparser::serialize_identifier(name, dest).expect("writing to a String");
                        dest.push(')');
                    }
                    ContentItem::Counter { name, style } => {
                        dest.push_str("counter(");
                        cssparser::serialize_identifier(name, dest).expect("writing to a String");
                        counter_style_argument(style, dest);
                        dest.push(')');
                    }
                    ContentItem::Counters {
                        name,
                        separator,
                        style,
                    } => {
                        dest.push_str("counters(");
                        cssparser::serialize_identifier(name, dest).expect("writing to a String");
                        dest.push_str(", ");
                        cssparser::serialize_string(separator, dest).expect("writing to a String");
                        counter_style_argument(style, dest);
                        dest.push(')');
                    }
                }
            }
        }
    }
}

/// A counter function's style after a comma, left out when it is
/// `decimal`, as Chrome writes it.
fn counter_style_argument(style: &ListStyleType, dest: &mut String) {
    if *style != ListStyleType::Decimal {
        dest.push_str(", ");
        list_style_type(style, dest);
    }
}

/// `list-style-type` and a counter style: a predefined style's name in
/// lowercase, another as written, a string quoted.
pub(crate) fn list_style_type(style: &ListStyleType, dest: &mut String) {
    let name = match style {
        ListStyleType::Disc => "disc",
        ListStyleType::Circle => "circle",
        ListStyleType::Square => "square",
        ListStyleType::DisclosureOpen => "disclosure-open",
        ListStyleType::DisclosureClosed => "disclosure-closed",
        ListStyleType::Decimal => "decimal",
        ListStyleType::DecimalLeadingZero => "decimal-leading-zero",
        ListStyleType::LowerRoman => "lower-roman",
        ListStyleType::UpperRoman => "upper-roman",
        ListStyleType::LowerGreek => "lower-greek",
        ListStyleType::LowerAlpha => "lower-alpha",
        ListStyleType::LowerLatin => "lower-latin",
        ListStyleType::UpperAlpha => "upper-alpha",
        ListStyleType::UpperLatin => "upper-latin",
        ListStyleType::None => "none",
        ListStyleType::Named(name) => {
            cssparser::serialize_identifier(name, dest).expect("writing to a String");
            return;
        }
        ListStyleType::String(string) => {
            cssparser::serialize_string(string, dest).expect("writing to a String");
            return;
        }
    };
    dest.push_str(name);
}

pub(crate) fn list_style_position_name(position: ListStylePosition) -> &'static str {
    match position {
        ListStylePosition::Outside => "outside",
        ListStylePosition::Inside => "inside",
    }
}

/// `counter-reset`: `none`, or each counter with its value (a
/// `reversed()` one without its own has none).
pub(crate) fn counter_reset(counters: &[CounterReset], dest: &mut String) {
    if counters.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, counter) in counters.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        if counter.reversed {
            dest.push_str("reversed(");
        }
        cssparser::serialize_identifier(&counter.name, dest).expect("writing to a String");
        if counter.reversed {
            dest.push(')');
        }
        if let Some(value) = counter.value {
            dest.push(' ');
            dest.push_str(&value.to_string());
        }
    }
}

/// `counter-increment` and `counter-set`: `none`, or each counter with
/// its value.
pub(crate) fn counter_changes(counters: &[CounterChange], dest: &mut String) {
    if counters.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, counter) in counters.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        cssparser::serialize_identifier(&counter.name, dest).expect("writing to a String");
        dest.push(' ');
        dest.push_str(&counter.value.to_string());
    }
}

pub(crate) fn clear_name(clear: Clear) -> &'static str {
    match clear {
        Clear::None => "none",
        Clear::Left => "left",
        Clear::Right => "right",
        Clear::Both => "both",
    }
}

pub(crate) fn position_name(position: Position) -> &'static str {
    match position {
        Position::Static => "static",
        Position::Relative => "relative",
        Position::Absolute => "absolute",
        Position::Fixed => "fixed",
        Position::Sticky => "sticky",
    }
}

pub(crate) fn display_name(display: Display) -> &'static str {
    match display {
        Display::Block => "block",
        Display::Inline => "inline",
        Display::Flex => "flex",
        Display::Grid => "grid",
        Display::InlineBlock => "inline-block",
        Display::InlineFlex => "inline-flex",
        Display::InlineGrid => "inline-grid",
        Display::FlowRoot => "flow-root",
        Display::WebkitBox => "-webkit-box",
        Display::WebkitInlineBox => "-webkit-inline-box",
        Display::Table(table) => match table {
            TableDisplay::Table => "table",
            TableDisplay::RowGroup => "table-row-group",
            TableDisplay::HeaderGroup => "table-header-group",
            TableDisplay::FooterGroup => "table-footer-group",
            TableDisplay::Row => "table-row",
            TableDisplay::Cell => "table-cell",
            TableDisplay::ColumnGroup => "table-column-group",
            TableDisplay::Column => "table-column",
            TableDisplay::Caption => "table-caption",
        },
        Display::InlineTable => "inline-table",
        Display::ListItem => "list-item",
        Display::Contents => "contents",
        Display::None => "none",
    }
}

pub(crate) fn box_sizing_name(box_sizing: BoxSizing) -> &'static str {
    match box_sizing {
        BoxSizing::ContentBox => "content-box",
        BoxSizing::BorderBox => "border-box",
    }
}

pub(crate) fn border_style_name(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::None => "none",
        BorderStyle::Hidden => "hidden",
        BorderStyle::Solid => "solid",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Double => "double",
        BorderStyle::Groove => "groove",
        BorderStyle::Ridge => "ridge",
        BorderStyle::Inset => "inset",
        BorderStyle::Outset => "outset",
    }
}

pub(crate) fn outline_style_name(style: computed_style::OutlineStyle) -> &'static str {
    match style {
        computed_style::OutlineStyle::Auto => "auto",
        computed_style::OutlineStyle::Line(style) => border_style_name(style),
    }
}

fn overflow_name(overflow: OverflowValue) -> &'static str {
    match overflow {
        OverflowValue::Visible => "visible",
        OverflowValue::Hidden => "hidden",
        OverflowValue::Clip => "clip",
        OverflowValue::Auto => "auto",
        OverflowValue::Scroll => "scroll",
    }
}

pub(crate) fn flex_direction_name(direction: FlexDirection) -> &'static str {
    match direction {
        FlexDirection::Row => "row",
        FlexDirection::RowReverse => "row-reverse",
        FlexDirection::Column => "column",
        FlexDirection::ColumnReverse => "column-reverse",
    }
}

pub(crate) fn flex_wrap_name(wrap: FlexWrap) -> &'static str {
    match wrap {
        FlexWrap::Nowrap => "nowrap",
        FlexWrap::Wrap => "wrap",
        FlexWrap::WrapReverse => "wrap-reverse",
    }
}

pub(crate) fn content_alignment_name(alignment: ContentAlignment) -> &'static str {
    match alignment {
        ContentAlignment::Normal => "normal",
        ContentAlignment::Start => "start",
        ContentAlignment::End => "end",
        ContentAlignment::FlexStart => "flex-start",
        ContentAlignment::FlexEnd => "flex-end",
        ContentAlignment::Center => "center",
        ContentAlignment::Stretch => "stretch",
        ContentAlignment::SpaceBetween => "space-between",
        ContentAlignment::SpaceAround => "space-around",
        ContentAlignment::SpaceEvenly => "space-evenly",
    }
}

pub(crate) fn item_alignment_name(alignment: ItemAlignment) -> &'static str {
    match alignment {
        ItemAlignment::Normal => "normal",
        ItemAlignment::Stretch => "stretch",
        ItemAlignment::Start => "start",
        ItemAlignment::End => "end",
        ItemAlignment::SelfStart => "self-start",
        ItemAlignment::SelfEnd => "self-end",
        ItemAlignment::FlexStart => "flex-start",
        ItemAlignment::FlexEnd => "flex-end",
        ItemAlignment::Center => "center",
        ItemAlignment::Baseline => "baseline",
    }
}

pub(crate) fn font_style_name(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
        FontStyle::Oblique => "oblique",
    }
}

pub(crate) fn text_align_name(align: TextAlign) -> &'static str {
    match align {
        TextAlign::Start => "start",
        TextAlign::End => "end",
        TextAlign::Left => "left",
        TextAlign::Right => "right",
        TextAlign::Center => "center",
    }
}

pub(crate) fn white_space_name(white_space: WhiteSpace) -> &'static str {
    match white_space {
        WhiteSpace::Normal => "normal",
        WhiteSpace::Pre => "pre",
        WhiteSpace::Nowrap => "nowrap",
        WhiteSpace::PreWrap => "pre-wrap",
        WhiteSpace::PreLine => "pre-line",
        WhiteSpace::BreakSpaces => "break-spaces",
    }
}

/// A keyword's name; a length is written as one.
pub(crate) fn vertical_align_name(align: VerticalAlign) -> &'static str {
    match align {
        VerticalAlign::Baseline => "baseline",
        VerticalAlign::Sub => "sub",
        VerticalAlign::Super => "super",
        VerticalAlign::TextTop => "text-top",
        VerticalAlign::TextBottom => "text-bottom",
        VerticalAlign::Middle => "middle",
        VerticalAlign::Top => "top",
        VerticalAlign::Bottom => "bottom",
        VerticalAlign::Length(_) => unreachable!("a length is written as one"),
    }
}

pub(crate) fn white_space_collapse_name(collapse: WhiteSpaceCollapse) -> &'static str {
    match collapse {
        WhiteSpaceCollapse::Collapse => "collapse",
        WhiteSpaceCollapse::Preserve => "preserve",
        WhiteSpaceCollapse::PreserveBreaks => "preserve-breaks",
        WhiteSpaceCollapse::BreakSpaces => "break-spaces",
    }
}

pub(crate) fn text_wrap_mode_name(mode: TextWrapMode) -> &'static str {
    match mode {
        TextWrapMode::Wrap => "wrap",
        TextWrapMode::Nowrap => "nowrap",
    }
}

pub(crate) fn appearance_name(appearance: computed_style::Appearance) -> &'static str {
    use computed_style::Appearance::*;
    match appearance {
        None => "none",
        Auto => "auto",
        Checkbox => "checkbox",
        Radio => "radio",
        Button => "button",
        Listbox => "listbox",
        Menulist => "menulist",
        MenulistButton => "menulist-button",
        Meter => "meter",
        ProgressBar => "progress-bar",
        Searchfield => "searchfield",
        Textarea => "textarea",
        Textfield => "textfield",
        BaseSelect => "base-select",
    }
}

pub(crate) fn box_orient_name(value: computed_style::BoxOrient) -> &'static str {
    match value {
        computed_style::BoxOrient::Horizontal => "horizontal",
        computed_style::BoxOrient::Vertical => "vertical",
    }
}

pub(crate) fn box_direction_name(value: computed_style::BoxDirection) -> &'static str {
    match value {
        computed_style::BoxDirection::Normal => "normal",
        computed_style::BoxDirection::Reverse => "reverse",
    }
}

pub(crate) fn box_pack_name(value: computed_style::BoxPack) -> &'static str {
    use computed_style::BoxPack;
    match value {
        BoxPack::Start => "start",
        BoxPack::Center => "center",
        BoxPack::End => "end",
        BoxPack::Justify => "justify",
    }
}

pub(crate) fn box_align_name(value: computed_style::BoxAlign) -> &'static str {
    use computed_style::BoxAlign;
    match value {
        BoxAlign::Stretch => "stretch",
        BoxAlign::Start => "start",
        BoxAlign::Center => "center",
        BoxAlign::End => "end",
        BoxAlign::Baseline => "baseline",
    }
}

pub(crate) fn object_fit_name(value: computed_style::ObjectFit) -> &'static str {
    use computed_style::ObjectFit;
    match value {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::None => "none",
        ObjectFit::ScaleDown => "scale-down",
    }
}

pub(crate) fn text_overflow_name(overflow: TextOverflow) -> &'static str {
    match overflow {
        TextOverflow::Clip => "clip",
        TextOverflow::Ellipsis => "ellipsis",
    }
}

pub(crate) fn text_transform_name(transform: TextTransform) -> &'static str {
    match transform {
        TextTransform::None => "none",
        TextTransform::Capitalize => "capitalize",
        TextTransform::Uppercase => "uppercase",
        TextTransform::Lowercase => "lowercase",
    }
}

pub(crate) fn word_break_name(word_break: WordBreak) -> &'static str {
    match word_break {
        WordBreak::Normal => "normal",
        WordBreak::BreakAll => "break-all",
        WordBreak::KeepAll => "keep-all",
        WordBreak::BreakWord => "break-word",
    }
}

pub(crate) fn overflow_wrap_name(wrap: OverflowWrap) -> &'static str {
    match wrap {
        OverflowWrap::Normal => "normal",
        OverflowWrap::BreakWord => "break-word",
        OverflowWrap::Anywhere => "anywhere",
    }
}

pub(crate) fn text_anchor_name(anchor: computed_style::TextAnchor) -> &'static str {
    match anchor {
        computed_style::TextAnchor::Start => "start",
        computed_style::TextAnchor::Middle => "middle",
        computed_style::TextAnchor::End => "end",
    }
}

pub(crate) fn fill_rule_name(rule: gfx::FillRule) -> &'static str {
    match rule {
        gfx::FillRule::NonZero => "nonzero",
        gfx::FillRule::EvenOdd => "evenodd",
    }
}

pub(crate) fn stroke_linecap_name(cap: StrokeLinecap) -> &'static str {
    match cap {
        StrokeLinecap::Butt => "butt",
        StrokeLinecap::Round => "round",
        StrokeLinecap::Square => "square",
    }
}

pub(crate) fn stroke_linejoin_name(join: StrokeLinejoin) -> &'static str {
    match join {
        StrokeLinejoin::Miter => "miter",
        StrokeLinejoin::Round => "round",
        StrokeLinejoin::Bevel => "bevel",
    }
}

/// Lowercase, as Chrome writes them (`visiblepainted`).
pub(crate) fn pointer_events_name(value: computed_style::PointerEvents) -> &'static str {
    use computed_style::PointerEvents::*;
    match value {
        Auto => "auto",
        None => "none",
        VisiblePainted => "visiblepainted",
        VisibleFill => "visiblefill",
        VisibleStroke => "visiblestroke",
        Visible => "visible",
        Painted => "painted",
        Fill => "fill",
        Stroke => "stroke",
        All => "all",
        BoundingBox => "bounding-box",
    }
}

pub(crate) fn visibility_name(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Visible => "visible",
        Visibility::Hidden => "hidden",
        Visibility::Collapse => "collapse",
    }
}

pub(crate) fn text_security_name(security: computed_style::TextSecurity) -> &'static str {
    use computed_style::TextSecurity::*;
    match security {
        None => "none",
        Disc => "disc",
        Circle => "circle",
        Square => "square",
    }
}

pub(crate) fn border_collapse_name(value: computed_style::BorderCollapse) -> &'static str {
    match value {
        computed_style::BorderCollapse::Separate => "separate",
        computed_style::BorderCollapse::Collapse => "collapse",
    }
}

pub(crate) fn table_layout_name(value: computed_style::TableLayout) -> &'static str {
    match value {
        computed_style::TableLayout::Auto => "auto",
        computed_style::TableLayout::Fixed => "fixed",
    }
}

pub(crate) fn caption_side_name(value: computed_style::CaptionSide) -> &'static str {
    match value {
        computed_style::CaptionSide::Top => "top",
        computed_style::CaptionSide::Bottom => "bottom",
    }
}

/// A number of percent: `50%`.
pub(crate) fn percent_to_css(percent: f32, dest: &mut String) {
    write_number(dest, percent.into());
    dest.push('%');
}

pub(crate) fn user_select_name(value: computed_style::UserSelect) -> &'static str {
    use computed_style::UserSelect::*;
    match value {
        Auto => "auto",
        Text => "text",
        None => "none",
        All => "all",
    }
}

pub(crate) fn user_modify_name(value: computed_style::UserModify) -> &'static str {
    use computed_style::UserModify::*;
    match value {
        ReadOnly => "read-only",
        ReadWrite => "read-write",
        ReadWritePlaintextOnly => "read-write-plaintext-only",
    }
}

pub(crate) fn hyphens_name(value: computed_style::Hyphens) -> &'static str {
    use computed_style::Hyphens::*;
    match value {
        None => "none",
        Manual => "manual",
        Auto => "auto",
    }
}

pub(crate) fn forced_color_adjust_name(value: computed_style::ForcedColorAdjust) -> &'static str {
    use computed_style::ForcedColorAdjust::*;
    match value {
        Auto => "auto",
        None => "none",
        PreserveParentColor => "preserve-parent-color",
    }
}

/// In lowercase, as Chrome writes them.
pub(crate) fn text_rendering_name(value: computed_style::TextRendering) -> &'static str {
    use computed_style::TextRendering::*;
    match value {
        Auto => "auto",
        OptimizeSpeed => "optimizespeed",
        OptimizeLegibility => "optimizelegibility",
        GeometricPrecision => "geometricprecision",
    }
}

pub(crate) fn font_smoothing_name(value: computed_style::FontSmoothing) -> &'static str {
    use computed_style::FontSmoothing::*;
    match value {
        Auto => "auto",
        None => "none",
        Antialiased => "antialiased",
        SubpixelAntialiased => "subpixel-antialiased",
    }
}

pub(crate) fn resize_name(value: computed_style::Resize) -> &'static str {
    use computed_style::Resize::*;
    match value {
        None => "none",
        Both => "both",
        Horizontal => "horizontal",
        Vertical => "vertical",
        Block => "block",
        Inline => "inline",
        Auto => "auto",
    }
}

pub(crate) fn overflow_anchor_name(value: computed_style::OverflowAnchor) -> &'static str {
    match value {
        computed_style::OverflowAnchor::Auto => "auto",
        computed_style::OverflowAnchor::None => "none",
    }
}

pub(crate) fn scroll_behavior_name(value: computed_style::ScrollBehavior) -> &'static str {
    match value {
        computed_style::ScrollBehavior::Auto => "auto",
        computed_style::ScrollBehavior::Smooth => "smooth",
    }
}

pub(crate) fn empty_cells_name(value: computed_style::EmptyCells) -> &'static str {
    match value {
        computed_style::EmptyCells::Show => "show",
        computed_style::EmptyCells::Hide => "hide",
    }
}
