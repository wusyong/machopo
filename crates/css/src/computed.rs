//! Computed values as Chrome writes them (Blink's
//! `CSSProperty::CSSValueFromComputedStyle` with `CSSValue::CssText`):
//! what `getComputedStyle` reads of a `ComputedStyle`. A property added
//! to the supported set adds its computed value here, next to its
//! declared value in `serialize.rs`.
//!
//! The values that depend on the box (`width`'s used size, say) are the
//! caller's: this takes only the style, and writes the computed value.

use base::numerics::clamp_round;
use computed_style::{
    ComputedStyle, LengthPercentage, LengthPercentageOrAuto, LengthPercentageOrNone, Overflow,
    Size, StyleColor, SvgPaint, SvgPaintFallback, TransformOperation,
};
use gfx::{Color, SizeF};

use crate::animation::{
    animation_name_to_css, easing_to_css, fill_mode_name, iteration_count_to_css, play_state_name,
    playback_direction_name, seconds_to_css, transition_behavior_name, transition_property_to_css,
};
use crate::background::{
    attachments_to_css, boxes_to_css, images_to_css, position_components_to_css, repeats_to_css,
    sizes_to_css,
};
use crate::font::font_family_to_css;
use crate::grid::{
    auto_flow_to_css, grid_line_to_css, template_areas_to_css, track_list_to_css,
    track_sizes_to_css,
};
use crate::properties::{LonghandId, Shorthand};
use crate::property_set::{CssPropertyValueSet, is_custom_property_name};
use crate::serialize::{
    border_collapse_name, border_style_name, box_sizing_name, caption_side_name, clear_name,
    content, content_alignment_name, counter_changes, counter_reset, display_name,
    empty_cells_name, fill_rule_name, flex_direction_name, flex_wrap_name, float_name,
    font_smoothing_name, font_style_name, forced_color_adjust_name, hyphens_name,
    item_alignment_name, list_style_position_name, list_style_type, outline_style_name,
    overflow_anchor_name, overflow_wrap_name, percent_to_css, pointer_events_name, position_name,
    resize_name, scroll_behavior_name, stroke_linecap_name, stroke_linejoin_name,
    table_layout_name, text_align_name, text_decoration_line_to_css, text_overflow_name,
    text_rendering_name, text_transform_name, text_wrap_mode_name, user_modify_name,
    user_select_name, vertical_align_name, visibility_name, white_space_collapse_name,
    word_break_name,
};
use crate::svg::{d_to_css, url_to_css};
use crate::values::write_number;

/// The longhands `getComputedStyle` lists, in name order, the prefixed
/// ones (`-webkit-`) last: what its `length` and `item()` go by (Blink's
/// `ComputableProperties`, which it keeps so). Shorthands aren't listed,
/// as Blink doesn't list them, though they are readable.
pub fn computable_properties() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = LonghandId::ALL.iter().map(|id| id.name()).collect();
    names.sort_unstable_by_key(|name| (name.starts_with('-'), *name));
    names
}

/// The custom properties `style` has, in no particular order: Chrome lists
/// them after the longhands (`CSSEnumeratedCustomProperties`), the ones
/// invalid at computed-value time too.
pub fn custom_property_names(style: &ComputedStyle) -> Vec<String> {
    style
        .variables
        .entries()
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect()
}

/// Whether a property's resolved value is the used value, so reading it
/// needs the box laid out (Blink's `CSSProperty::IsLayoutDependent`).
pub fn is_layout_dependent(property: &str) -> bool {
    matches!(
        LonghandId::from_name(property).map(LonghandId::physical),
        Some(
            LonghandId::Top
                | LonghandId::Right
                | LonghandId::Bottom
                | LonghandId::Left
                | LonghandId::Width
                | LonghandId::Height
                | LonghandId::MarginTop
                | LonghandId::MarginRight
                | LonghandId::MarginBottom
                | LonghandId::MarginLeft
                | LonghandId::PaddingTop
                | LonghandId::PaddingRight
                | LonghandId::PaddingBottom
                | LonghandId::PaddingLeft
                | LonghandId::Transform
                | LonghandId::TransformOrigin
        )
    )
}

/// The physical longhand's name for a flow-relative one (`margin-left`
/// for `margin-inline-start`), whose resolved value is that one's (Blink's
/// `CSSProperty::ResolveDirectionAwareProperty`); `None` for any other
/// name.
pub fn physical_property(property: &str) -> Option<&'static str> {
    let id = LonghandId::from_name(property)?;
    (id.physical() != id).then(|| id.physical().name())
}

/// `transform`'s resolved value: its functions as one matrix, the
/// percentages against `size`, the element's border box, as Chrome writes
/// it (Blink's `ComputedStyleUtils::ComputedTransform`, which excludes
/// `transform-origin` too). `none` for an element with no transform; an
/// element with no box resolves to `none` in Chrome, which is the caller's
/// to write, having the box.
pub fn resolved_transform(style: &ComputedStyle, size: SizeF) -> String {
    if style.transform.is_empty() {
        return "none".to_owned();
    }
    let matrix = style.transform_matrix(size);
    let mut out = String::from("matrix(");
    for (i, value) in matrix.0.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_number(&mut out, (*value).into());
    }
    out.push(')');
    out
}

/// `transform-origin`'s resolved value: the point in px, its percentages
/// of `reference`, the reference box's size (Blink's
/// `ComputedStyleUtils::ValueForTransformOrigin`), and the depth if it
/// isn't 0.
pub fn resolved_transform_origin(style: &ComputedStyle, reference: SizeF) -> String {
    let origin = style.rare.transform_origin;
    let mut out = String::new();
    for (i, value) in [
        origin.x.resolve(reference.width),
        origin.y.resolve(reference.height),
        origin.z,
    ]
    .into_iter()
    .enumerate()
    {
        if i == 2 && value == 0.0 {
            break;
        }
        if i > 0 {
            out.push(' ');
        }
        write_number(&mut out, value.into());
        out.push_str("px");
    }
    out
}

/// `property`'s computed value in `style`, as Chrome writes it; `None` if
/// it isn't a longhand we support or a custom property. A custom property
/// with no value (not set, or invalid at computed-value time) is the
/// empty string. The caller substitutes the used value for the properties
/// whose resolved value is one (`width` and its kin), and assembles the
/// shorthands ([`shorthand_longhands`] and [`shorthand_value`]).
pub fn computed_value(style: &ComputedStyle, property: &str) -> Option<String> {
    if is_custom_property_name(property) {
        return Some(
            style
                .variables
                .get(property)
                .map_or_else(String::new, |value| value.to_string()),
        );
    }
    let id = LonghandId::from_name(property)?;
    let mut out = String::new();
    longhand_to_css(style, id, &mut out);
    Some(out)
}

/// The computed value of the shorthand `property` that Chrome writes from
/// the style itself, not its longhands' resolved values (Blink's
/// `CSSValueFromComputedStyleInternal` of the shorthand): `text-
/// decoration`, its values that aren't initial (the color left out while
/// it is `currentcolor`, which its resolved value doesn't tell), else
/// `none`. `None` for the other shorthands.
pub fn shorthand_from_style(style: &ComputedStyle, property: &str) -> Option<String> {
    if Shorthand::from_name(property)? != Shorthand::TextDecoration {
        return None;
    }
    let mut parts = Vec::new();
    let mut part = |write: &dyn Fn(&mut String)| {
        let mut text = String::new();
        write(&mut text);
        parts.push(text);
    };
    if !style.text_decoration_line.is_none() {
        part(&|dest| text_decoration_line_to_css(style.text_decoration_line, dest));
    }
    let rare = &style.rare;
    if rare.text_decoration_thickness != computed_style::TextDecorationThickness::Auto {
        part(&|dest| {
            crate::text_decor::computed_thickness_to_css(rare.text_decoration_thickness, dest)
        });
    }
    if rare.text_decoration_style != computed_style::TextDecorationStyle::Solid {
        part(&|dest| {
            dest.push_str(crate::text_decor::text_decoration_style_name(
                rare.text_decoration_style,
            ))
        });
    }
    if rare.text_decoration_color != StyleColor::CurrentColor {
        part(&|dest| style_color_to_css(style, rare.text_decoration_color, dest));
    }
    if parts.is_empty() {
        parts.push("none".to_owned());
    }
    Some(parts.join(" "))
}

/// The longhands `property`, a shorthand, sets; `None` if it isn't a
/// shorthand we support. `getComputedStyle` reads a shorthand by resolving
/// each of these and writing them back with [`shorthand_value`], as Blink
/// does (`StylePropertySerializer` over the computed style).
pub fn shorthand_longhands(property: &str) -> Option<Vec<&'static str>> {
    let shorthand = Shorthand::from_name(property)?;
    Some(
        shorthand
            .longhands()
            .into_iter()
            .map(LonghandId::name)
            .collect(),
    )
}

/// The shorthand `property` written from `values`, its longhands' values
/// by name (the serialization of a declaration block, through one):
/// `None` if they can't be written as it, which is what a shorthand of
/// Chrome's that also sets longhands machopo hasn't (`text-decoration`) always
/// comes to. `background` is written as Chrome's
/// resolved value is, every value of every layer; `border` from its
/// sides, `border-image` as Chrome writes it.
pub fn shorthand_value(property: &str, values: &[(&str, String)]) -> Option<String> {
    let mut block = CssPropertyValueSet::default();
    for (name, value) in values {
        // Chrome's `font` leaves out the weight while it is 400, as it
        // does `normal`.
        let value = if *name == "font-weight" && value == "400" {
            "normal"
        } else {
            value
        };
        block.set_property(name, value, "");
    }
    match Shorthand::from_name(property) {
        Some(Shorthand::Background) => return block.background_computed_text(),
        // Chrome's `text-wrap` also has `text-wrap-style`, left out of the
        // value while it is `auto`: the mode alone is what it writes.
        Some(Shorthand::TextWrap) => return values.first().map(|(_, value)| value.clone()),
        // Chrome writes `border` from the sides alone, whatever the
        // `border-image` (Blink's `Border::CSSValueFromComputedStyle`).
        Some(Shorthand::Border) => {
            for (name, value) in crate::properties::BORDER_IMAGE
                .iter()
                .zip(crate::border_image::initial_values())
            {
                let mut text = String::new();
                crate::serialize::value_to_css(&value, &mut text);
                block.set_property(name.name(), &text, "");
            }
        }
        // `none` without an image, else every value (Blink's
        // `ValueForNinePieceImage`).
        Some(Shorthand::BorderImage) => {
            let [source, slice, width, outset, repeat] = values else {
                return None;
            };
            return Some(if source.1 == "none" {
                "none".to_owned()
            } else {
                format!(
                    "{} {} / {} / {} {}",
                    source.1, slice.1, width.1, outset.1, repeat.1
                )
            });
        }
        _ => {}
    }
    let text = block.property_value(property);
    (!text.is_empty()).then_some(text)
}

/// `length`, as a computed length or percentage is written: `10px`, `50%`.
pub(crate) fn length_percentage(value: LengthPercentage, dest: &mut String) {
    match value {
        LengthPercentage::Px(px) => px_to_css(px, dest),
        LengthPercentage::Percent(percent) => {
            write_number(dest, percent.into());
            dest.push('%');
        }
        LengthPercentage::Calc { px, percent, .. } => {
            crate::calc::write_computed(px, percent, dest);
        }
    }
}

/// A number as CSS writes it (no trailing zeros), for a caller that
/// writes a used value of its own (`dom_api`'s resolved values).
pub fn write_css_number(value: f64) -> String {
    let mut out = String::new();
    write_number(&mut out, value);
    out
}

pub(crate) fn px_to_css(px: f32, dest: &mut String) {
    write_number(dest, px.into());
    dest.push_str("px");
}

/// A resolved color: `rgb(r, g, b)`, or `rgba(r, g, b, a)` when it isn't
/// opaque (Blink's `CSSColor::SerializeAsCSSComponentValue`).
fn color_to_css(color: Color, dest: &mut String) {
    let (r, g, b) = (color.r, color.g, color.b);
    if color.a == u8::MAX {
        dest.push_str(&format!("rgb({r}, {g}, {b})"));
        return;
    }
    dest.push_str(&format!("rgba({r}, {g}, {b}, "));
    dest.push_str(&alpha_to_css(color.a));
    dest.push(')');
}

/// The shortest alpha that comes back to the same byte (CSS Color 4's
/// "serializing alpha values"): 128 is `0.502`, not `0.501961`.
pub(crate) fn alpha_to_css(alpha: u8) -> String {
    for places in 0..3 {
        let scale = 10f64.powi(places);
        let rounded = (f64::from(alpha) / 255.0 * scale).round() / scale;
        if clamp_round::<u8>(rounded * 255.0) == alpha {
            let mut out = String::new();
            write_number(&mut out, rounded);
            return out;
        }
    }
    let mut out = String::new();
    write_number(
        &mut out,
        (f64::from(alpha) / 255.0 * 1000.0).round() / 1000.0,
    );
    out
}

/// `currentcolor` resolves to the element's own `color` (Blink's
/// `ComputedStyleUtils::CurrentColorOrValidColor`).
fn style_color_to_css(style: &ComputedStyle, value: StyleColor, dest: &mut String) {
    match value {
        StyleColor::CurrentColor => color_to_css(style.color, dest),
        StyleColor::Rgba(color) => color_to_css(color, dest),
    }
}

fn size_to_css(size: Size, dest: &mut String) {
    match size {
        Size::Auto => dest.push_str("auto"),
        Size::LengthPercentage(value) => length_percentage(value, dest),
        Size::MinContent => dest.push_str("min-content"),
        Size::MaxContent => dest.push_str("max-content"),
        Size::FitContent => dest.push_str("fit-content"),
    }
}

fn overflow_name(overflow: Overflow) -> &'static str {
    match overflow {
        Overflow::Visible => "visible",
        Overflow::Clip => "clip",
        Overflow::Hidden => "hidden",
        Overflow::Auto => "auto",
        Overflow::Scroll => "scroll",
    }
}

/// The transform functions, computed: lengths absolute, percentages left
/// for the box to resolve. Chrome writes a `matrix()` here, which needs
/// the box's size; the caller with a box makes it.
fn transform_to_css(functions: &[TransformOperation], dest: &mut String) {
    if functions.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, function) in functions.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        match function {
            TransformOperation::Translate(x, y) => {
                dest.push_str("translate(");
                length_percentage(*x, dest);
                dest.push_str(", ");
                length_percentage(*y, dest);
                dest.push(')');
            }
            TransformOperation::Scale(x, y) => {
                dest.push_str("scale(");
                write_number(dest, (*x).into());
                dest.push_str(", ");
                write_number(dest, (*y).into());
                dest.push(')');
            }
            TransformOperation::Rotate(degrees) => {
                dest.push_str("rotate(");
                write_number(dest, (*degrees).into());
                dest.push_str("deg)");
            }
            TransformOperation::SkewX(x) => {
                dest.push_str("skewX(");
                write_number(dest, (*x).into());
                dest.push_str("deg)");
            }
            TransformOperation::SkewY(y) => {
                dest.push_str("skewY(");
                write_number(dest, (*y).into());
                dest.push_str("deg)");
            }
            TransformOperation::Skew(x, y) => {
                dest.push_str("skew(");
                write_number(dest, (*x).into());
                dest.push_str("deg, ");
                write_number(dest, (*y).into());
                dest.push_str("deg)");
            }
            // Only a box resolves it; with none, its percentages are of
            // nothing.
            TransformOperation::Interpolated { .. } => {
                let m = function.matrix(gfx::SizeF::default()).0;
                dest.push_str("matrix(");
                for (j, value) in m.iter().enumerate() {
                    if j > 0 {
                        dest.push_str(", ");
                    }
                    write_number(dest, (*value).into());
                }
                dest.push(')');
            }
            TransformOperation::Matrix(m) => {
                dest.push_str("matrix(");
                for (j, value) in m.iter().enumerate() {
                    if j > 0 {
                        dest.push_str(", ");
                    }
                    write_number(dest, (*value).into());
                }
                dest.push(')');
            }
        }
    }
}

fn longhand_to_css(style: &ComputedStyle, id: LonghandId, dest: &mut String) {
    use LonghandId::*;

    let auto_or = |value: LengthPercentageOrAuto, dest: &mut String| match value {
        LengthPercentageOrAuto::Auto => dest.push_str("auto"),
        LengthPercentageOrAuto::LengthPercentage(value) => length_percentage(value, dest),
    };
    // A flow-relative longhand's value is its physical one's.
    match id.physical() {
        crate::properties::logical_longhands!() => unreachable!("mapped to a physical longhand"),
        Display => dest.push_str(display_name(style.display)),
        Position => dest.push_str(position_name(style.position)),
        Float => dest.push_str(float_name(style.float)),
        Clear => dest.push_str(clear_name(style.clear)),
        Content => content(&style.rare.content, dest),
        CounterReset => counter_reset(&style.rare.counter_reset, dest),
        CounterIncrement => counter_changes(&style.rare.counter_increment, dest),
        CounterSet => counter_changes(&style.rare.counter_set, dest),
        ListStyleType => list_style_type(&style.rare_inherited.list_style_type, dest),
        ListStylePosition => dest.push_str(list_style_position_name(style.list_style_position)),
        ListStyleImage => dest.push_str("none"),
        // The computed value; Chrome's resolved value for a positioned box
        // is the used offset, which needs the box.
        Top => auto_or(style.inset.top, dest),
        Right => auto_or(style.inset.right, dest),
        Bottom => auto_or(style.inset.bottom, dest),
        Left => auto_or(style.inset.left, dest),
        BoxSizing => dest.push_str(box_sizing_name(style.box_sizing)),
        Width => size_to_css(style.width, dest),
        Height => size_to_css(style.height, dest),
        MinWidth => auto_or(style.min_width, dest),
        MinHeight => auto_or(style.min_height, dest),
        MaxWidth => none_or(style.max_width, dest),
        MaxHeight => none_or(style.max_height, dest),
        MarginTop => auto_or(style.margin.top, dest),
        MarginRight => auto_or(style.margin.right, dest),
        MarginBottom => auto_or(style.margin.bottom, dest),
        MarginLeft => auto_or(style.margin.left, dest),
        PaddingTop => length_percentage(style.padding.top, dest),
        PaddingRight => length_percentage(style.padding.right, dest),
        PaddingBottom => length_percentage(style.padding.bottom, dest),
        PaddingLeft => length_percentage(style.padding.left, dest),
        BorderTopWidth => px_to_css(style.border_width.top, dest),
        BorderRightWidth => px_to_css(style.border_width.right, dest),
        BorderBottomWidth => px_to_css(style.border_width.bottom, dest),
        BorderLeftWidth => px_to_css(style.border_width.left, dest),
        BorderTopStyle => dest.push_str(border_style_name(style.border_style.top)),
        BorderRightStyle => dest.push_str(border_style_name(style.border_style.right)),
        BorderBottomStyle => dest.push_str(border_style_name(style.border_style.bottom)),
        BorderLeftStyle => dest.push_str(border_style_name(style.border_style.left)),
        BorderTopColor => style_color_to_css(style, style.border_color.top, dest),
        BorderRightColor => style_color_to_css(style, style.border_color.right, dest),
        BorderBottomColor => style_color_to_css(style, style.border_color.bottom, dest),
        BorderLeftColor => style_color_to_css(style, style.border_color.left, dest),
        BorderTopLeftRadius => corner_radius_to_css(style.rare.border_radius.top_left, dest),
        BorderTopRightRadius => corner_radius_to_css(style.rare.border_radius.top_right, dest),
        BorderBottomRightRadius => {
            corner_radius_to_css(style.rare.border_radius.bottom_right, dest)
        }
        BorderBottomLeftRadius => corner_radius_to_css(style.rare.border_radius.bottom_left, dest),
        BoxShadow => shadows_to_css(style, &style.box_shadow, true, dest),
        OutlineColor => style_color_to_css(style, style.outline_color, dest),
        OutlineStyle => dest.push_str(outline_style_name(style.outline_style)),
        OutlineWidth => px_to_css(style.outline_width, dest),
        OutlineOffset => px_to_css(style.outline_offset, dest),
        BackgroundColor => style_color_to_css(style, style.background_color, dest),
        BackgroundImage => images_to_css(style, dest),
        BackgroundPositionX => {
            position_components_to_css(&style.backgrounds.background_position_x, dest)
        }
        BackgroundPositionY => {
            position_components_to_css(&style.backgrounds.background_position_y, dest)
        }
        BackgroundSize => sizes_to_css(&style.backgrounds.background_size, dest),
        BackgroundRepeat => repeats_to_css(&style.backgrounds.background_repeat, dest),
        BackgroundAttachment => attachments_to_css(&style.backgrounds.background_attachment, dest),
        BackgroundClip => boxes_to_css(&style.backgrounds.background_clip, dest),
        BackgroundOrigin => boxes_to_css(&style.backgrounds.background_origin, dest),
        TextDecorationLine => text_decoration_line_to_css(style.text_decoration_line, dest),
        TextDecorationStyle => dest.push_str(crate::text_decor::text_decoration_style_name(
            style.rare.text_decoration_style,
        )),
        TextDecorationColor => style_color_to_css(style, style.rare.text_decoration_color, dest),
        TextDecorationThickness => {
            crate::text_decor::computed_thickness_to_css(style.rare.text_decoration_thickness, dest)
        }
        TextUnderlineOffset => match style.rare_inherited.text_underline_offset {
            None => dest.push_str("auto"),
            Some(offset) => length_percentage(offset, dest),
        },
        TextUnderlinePosition => crate::text_decor::underline_position_to_css(
            style.rare_inherited.text_underline_position,
            dest,
        ),
        TextShadow => shadows_to_css(style, &style.rare_inherited.text_shadow, false, dest),
        WebkitTextFillColor => {
            style_color_to_css(style, style.rare_inherited.text_fill_color, dest)
        }
        Filter => {
            crate::filter::computed_filter_to_css(&style.rare.filter, dest, &|color, dest| {
                style_color_to_css(style, color, dest)
            })
        }
        Isolation => dest.push_str(crate::filter::isolation_name(style.rare.isolation)),
        Opacity => write_number(dest, style.opacity.into()),
        OverflowX => dest.push_str(overflow_name(style.overflow_x)),
        OverflowY => dest.push_str(overflow_name(style.overflow_y)),
        Transform => transform_to_css(&style.transform, dest),
        TransformOrigin => {
            let origin = style.rare.transform_origin;
            length_percentage(origin.x, dest);
            dest.push(' ');
            length_percentage(origin.y, dest);
            if origin.z != 0.0 {
                dest.push(' ');
                write_number(dest, origin.z.into());
                dest.push_str("px");
            }
        }
        TransformBox => dest.push_str(crate::effects::transform_box_name(style.rare.transform_box)),
        ObjectFit => dest.push_str(crate::serialize::object_fit_name(style.rare.object_fit)),
        ObjectPosition => {
            let position = style.rare.object_position;
            crate::background::position_component_to_css(position.x, dest);
            dest.push(' ');
            crate::background::position_component_to_css(position.y, dest);
        }
        WebkitBoxOrient => dest.push_str(crate::serialize::box_orient_name(style.rare.box_orient)),
        WebkitBoxDirection => dest.push_str(crate::serialize::box_direction_name(
            style.rare.box_direction,
        )),
        WebkitBoxPack => dest.push_str(crate::serialize::box_pack_name(style.rare.box_pack)),
        WebkitBoxAlign => dest.push_str(crate::serialize::box_align_name(style.rare.box_align)),
        WebkitBoxFlex => write_number(dest, style.rare.box_flex.into()),
        WebkitBoxOrdinalGroup => dest.push_str(&style.rare.box_ordinal_group.to_string()),
        Contain => crate::properties::contain_to_css(style.rare.contain, true, dest),
        Clip => crate::masking::computed_clip_to_css(style.rare.clip.as_ref(), dest),
        ClipPath => crate::masking::computed_clip_path_to_css(&style.rare.clip_path, dest),
        WebkitLineClamp => match style.rare.line_clamp {
            None => dest.push_str("none"),
            Some(lines) => dest.push_str(&lines.to_string()),
        },
        FlexDirection => dest.push_str(flex_direction_name(style.flex_direction)),
        FlexWrap => dest.push_str(flex_wrap_name(style.flex_wrap)),
        FlexGrow => write_number(dest, style.flex_grow.into()),
        FlexShrink => write_number(dest, style.flex_shrink.into()),
        FlexBasis => match style.flex_basis {
            computed_style::FlexBasis::Content => dest.push_str("content"),
            computed_style::FlexBasis::Size(size) => size_to_css(size, dest),
        },
        Order => dest.push_str(&style.order.to_string()),
        JustifyContent => dest.push_str(content_alignment_name(style.justify_content)),
        AlignContent => dest.push_str(content_alignment_name(style.align_content)),
        AlignItems => dest.push_str(item_alignment_name(style.align_items)),
        AlignSelf => match style.align_self {
            None => dest.push_str("auto"),
            Some(alignment) => dest.push_str(item_alignment_name(alignment)),
        },
        RowGap => length_percentage(style.row_gap, dest),
        ColumnGap => length_percentage(style.column_gap, dest),
        GridTemplateColumns => track_list_to_css(&style.rare.grid_template_columns, dest),
        GridTemplateRows => track_list_to_css(&style.rare.grid_template_rows, dest),
        GridAutoColumns => track_sizes_to_css(&style.rare.grid_auto_columns, dest),
        GridAutoRows => track_sizes_to_css(&style.rare.grid_auto_rows, dest),
        GridAutoFlow => auto_flow_to_css(style.rare.grid_auto_flow, dest),
        GridTemplateAreas => template_areas_to_css(style.rare.grid_template_areas.as_deref(), dest),
        GridRowStart => grid_line_to_css(&style.rare.grid_row_start, dest),
        GridRowEnd => grid_line_to_css(&style.rare.grid_row_end, dest),
        GridColumnStart => grid_line_to_css(&style.rare.grid_column_start, dest),
        GridColumnEnd => grid_line_to_css(&style.rare.grid_column_end, dest),
        JustifyItems => dest.push_str(item_alignment_name(style.justify_items)),
        JustifySelf => match style.justify_self {
            None => dest.push_str("auto"),
            Some(alignment) => dest.push_str(item_alignment_name(alignment)),
        },
        ZIndex => match style.z_index {
            None => dest.push_str("auto"),
            Some(index) => dest.push_str(&index.to_string()),
        },
        Color => color_to_css(style.color, dest),
        FontFamily => font_family_to_css(&style.font.family, dest),
        FontSize => px_to_css(style.font.size, dest),
        FontWeight => write_number(dest, style.font.weight.into()),
        FontStyle => dest.push_str(font_style_name(style.font.style)),
        FontStretch => crate::serialize::percent_to_css(style.font.stretch, dest),
        FontVariantLigatures => {
            crate::font_features::ligatures_to_css(style.font.variant_ligatures, dest)
        }
        FontVariantNumeric => {
            crate::font_features::numeric_to_css(style.font.variant_numeric, dest)
        }
        FontVariantEastAsian => {
            crate::font_features::east_asian_to_css(style.font.variant_east_asian, dest)
        }
        FontVariantCaps => dest.push_str(crate::font_features::caps_name(style.font.variant_caps)),
        FontVariantAlternates => {
            crate::font_features::alternates_to_css(&style.font.variant_alternates, dest)
        }
        FontVariantPosition => dest.push_str(crate::font_features::position_name(
            style.font.variant_position,
        )),
        FontVariantEmoji => {
            dest.push_str(crate::font_features::emoji_name(style.font.variant_emoji))
        }
        // `from-font` stays: machopo doesn't read the font's metric (Chrome
        // writes it).
        FontSizeAdjust => crate::font_features::size_adjust_to_css(style.font.size_adjust, dest),
        FontLanguageOverride => crate::font_features::language_override_to_css(
            style.font.language_override.as_deref(),
            dest,
        ),
        FontKerning => dest.push_str(crate::font_features::kerning_name(style.font.kerning)),
        FontOpticalSizing => dest.push_str(crate::font_features::auto_or_none(
            style.font.optical_sizing,
        )),
        FontFeatureSettings => {
            crate::font_features::feature_settings_to_css(&style.font.feature_settings, dest)
        }
        FontVariationSettings => {
            crate::font_features::variation_settings_to_css(&style.font.variation_settings, dest)
        }
        FontSynthesisWeight => dest.push_str(crate::font_features::auto_or_none(
            style.font.synthesis_weight,
        )),
        FontSynthesisStyle => dest.push_str(crate::font_features::auto_or_none(
            style.font.synthesis_style,
        )),
        FontSynthesisSmallCaps => dest.push_str(crate::font_features::auto_or_none(
            style.font.synthesis_small_caps,
        )),
        // A number resolves against the font size here, as Chrome's does
        // (the spec leaves the number as the computed value): finite, as
        // Chrome's float is (`3.40282e+38px`), not as layout clamps it.
        LineHeight => match style.line_height {
            computed_style::LineHeight::Normal => dest.push_str("normal"),
            computed_style::LineHeight::Number(number) => {
                px_to_css(crate::values::finite(number * style.font.size), dest)
            }
            computed_style::LineHeight::Px(px) => px_to_css(px, dest),
        },
        TextAlign => dest.push_str(text_align_name(style.text_align)),
        VerticalAlign => match style.vertical_align {
            computed_style::VerticalAlign::Length(length) => length_percentage(length, dest),
            keyword => dest.push_str(vertical_align_name(keyword)),
        },
        WhiteSpaceCollapse => dest.push_str(white_space_collapse_name(style.white_space_collapse)),
        TextWrapMode => dest.push_str(text_wrap_mode_name(style.text_wrap_mode)),
        TextTransform => dest.push_str(text_transform_name(style.text_transform)),
        TextOverflow => dest.push_str(text_overflow_name(style.text_overflow)),
        Appearance => dest.push_str(crate::serialize::appearance_name(style.appearance)),
        LetterSpacing => match style.rare_inherited.letter_spacing {
            None => dest.push_str("normal"),
            Some(px) => px_to_css(px, dest),
        },
        WordSpacing => px_to_css(style.rare_inherited.word_spacing, dest),
        TabSize => match style.rare_inherited.tab_size {
            computed_style::TabSize::Spaces(spaces) => write_number(dest, spaces.into()),
            computed_style::TabSize::Px(px) => px_to_css(px, dest),
        },
        TextIndent => length_percentage(style.rare_inherited.text_indent, dest),
        WordBreak => dest.push_str(word_break_name(style.rare_inherited.word_break)),
        OverflowWrap => dest.push_str(overflow_wrap_name(style.rare_inherited.overflow_wrap)),
        Fill => paint_to_css(style, &style.svg.fill, dest),
        Stroke => paint_to_css(style, &style.svg.stroke, dest),
        FillOpacity => write_number(dest, style.svg.fill_opacity.into()),
        StrokeOpacity => write_number(dest, style.svg.stroke_opacity.into()),
        StopColor => style_color_to_css(style, style.svg.stop_color, dest),
        StopOpacity => write_number(dest, style.svg.stop_opacity.into()),
        FillRule => dest.push_str(fill_rule_name(style.svg.fill_rule)),
        ClipRule => dest.push_str(fill_rule_name(style.svg.clip_rule)),
        TextAnchor => dest.push_str(crate::serialize::text_anchor_name(style.svg.text_anchor)),
        StrokeWidth => length_percentage(style.svg.stroke_width, dest),
        StrokeLinecap => dest.push_str(stroke_linecap_name(style.svg.stroke_linecap)),
        StrokeLinejoin => dest.push_str(stroke_linejoin_name(style.svg.stroke_linejoin)),
        StrokeMiterlimit => write_number(dest, style.svg.stroke_miterlimit.into()),
        StrokeDasharray => {
            if style.svg.stroke_dasharray.is_empty() {
                dest.push_str("none");
            }
            for (i, dash) in style.svg.stroke_dasharray.iter().enumerate() {
                if i > 0 {
                    dest.push_str(", ");
                }
                length_percentage(*dash, dest);
            }
        }
        StrokeDashoffset => length_percentage(style.svg.stroke_dashoffset, dest),
        Cx => length_percentage(style.svg.cx, dest),
        Cy => length_percentage(style.svg.cy, dest),
        R => length_percentage(style.svg.r, dest),
        Rx => auto_or(style.svg.rx, dest),
        Ry => auto_or(style.svg.ry, dest),
        X => length_percentage(style.svg.x, dest),
        Y => length_percentage(style.svg.y, dest),
        D => d_to_css(style.svg.d.as_deref(), dest),
        Visibility => dest.push_str(visibility_name(style.visibility)),
        WebkitTextSecurity => dest.push_str(crate::serialize::text_security_name(
            style.rare_inherited.text_security,
        )),
        PointerEvents => dest.push_str(pointer_events_name(style.pointer_events)),
        BorderCollapse => dest.push_str(border_collapse_name(style.border_collapse)),
        WebkitBorderHorizontalSpacing => {
            px_to_css(style.rare_inherited.border_spacing.horizontal, dest)
        }
        WebkitBorderVerticalSpacing => {
            px_to_css(style.rare_inherited.border_spacing.vertical, dest)
        }
        TableLayout => dest.push_str(table_layout_name(style.table_layout)),
        CaptionSide => dest.push_str(caption_side_name(style.caption_side)),
        EmptyCells => dest.push_str(empty_cells_name(style.empty_cells)),
        UserSelect => dest.push_str(user_select_name(style.rare_inherited.user_select)),
        WebkitUserModify => dest.push_str(user_modify_name(style.rare_inherited.user_modify)),
        Hyphens => dest.push_str(hyphens_name(style.rare_inherited.hyphens)),
        ForcedColorAdjust => dest.push_str(forced_color_adjust_name(
            style.rare_inherited.forced_color_adjust,
        )),
        WebkitTapHighlightColor => {
            style_color_to_css(style, style.rare_inherited.tap_highlight_color, dest)
        }
        // `auto` reads back as `currentcolor` does (Chrome).
        CaretColor => color_to_css(style.rare_inherited.caret_color.resolve(style.color), dest),
        TextSizeAdjust => match style.rare_inherited.text_size_adjust {
            computed_style::TextSizeAdjust::Auto => dest.push_str("auto"),
            computed_style::TextSizeAdjust::Percent(percent) => percent_to_css(percent, dest),
        },
        TextRendering => dest.push_str(text_rendering_name(style.font.text_rendering)),
        WebkitFontSmoothing => dest.push_str(font_smoothing_name(style.font.smoothing)),
        // Chrome's `auto` is `both` where text areas can be resized: always.
        Resize => dest.push_str(match style.rare.resize {
            computed_style::Resize::Auto => "both",
            resize => resize_name(resize),
        }),
        ScrollBehavior => dest.push_str(scroll_behavior_name(style.rare.scroll_behavior)),
        OverflowAnchor => dest.push_str(overflow_anchor_name(style.rare.overflow_anchor)),
        Cursor => crate::cursor::to_css(&style.rare_inherited.cursor, dest),
        BorderImageSource => crate::border_image::source_to_css(style, dest),
        BorderImageSlice => crate::border_image::slice_to_css(&style.rare.border_image.slice, dest),
        BorderImageWidth => crate::border_image::width_to_css(&style.rare.border_image.width, dest),
        BorderImageOutset => {
            crate::border_image::outset_to_css(&style.rare.border_image.outset, dest)
        }
        BorderImageRepeat => {
            let (horizontal, vertical) = style.rare.border_image.repeat;
            crate::border_image::repeat_to_css(horizontal, vertical, dest)
        }
        TransitionProperty => list_to_css(
            &style.transitions.property,
            dest,
            transition_property_to_css,
        ),
        TransitionDuration => list_to_css(&style.transitions.duration, dest, |t, dest| {
            seconds_to_css(*t, dest)
        }),
        TransitionTimingFunction => {
            list_to_css(&style.transitions.timing_function, dest, easing_to_css)
        }
        TransitionDelay => list_to_css(&style.transitions.delay, dest, |t, dest| {
            seconds_to_css(*t, dest)
        }),
        TransitionBehavior => list_to_css(&style.transitions.behavior, dest, |b, dest| {
            dest.push_str(transition_behavior_name(*b))
        }),
        AnimationDuration => list_to_css(&style.rare.animations.duration, dest, |t, dest| {
            seconds_to_css(*t, dest)
        }),
        AnimationTimingFunction => {
            list_to_css(&style.rare.animations.timing_function, dest, easing_to_css)
        }
        AnimationDelay => list_to_css(&style.rare.animations.delay, dest, |t, dest| {
            seconds_to_css(*t, dest)
        }),
        AnimationIterationCount => {
            list_to_css(&style.rare.animations.iteration_count, dest, |c, dest| {
                iteration_count_to_css(*c, dest)
            })
        }
        AnimationDirection => list_to_css(&style.rare.animations.direction, dest, |d, dest| {
            dest.push_str(playback_direction_name(*d))
        }),
        AnimationFillMode => list_to_css(&style.rare.animations.fill_mode, dest, |f, dest| {
            dest.push_str(fill_mode_name(*f))
        }),
        AnimationPlayState => list_to_css(&style.rare.animations.play_state, dest, |s, dest| {
            dest.push_str(play_state_name(*s))
        }),
        AnimationName => list_to_css(&style.rare.animations.name, dest, animation_name_to_css),
        AnimationTimeline => list_to_css(
            &style.rare.animations.timeline,
            dest,
            crate::animation::animation_timeline_to_css,
        ),
        AnimationRangeStart => list_to_css(
            &style.rare.animations.range_start,
            dest,
            crate::animation::animation_range_boundary_to_css,
        ),
        AnimationRangeEnd => list_to_css(
            &style.rare.animations.range_end,
            dest,
            crate::animation::animation_range_boundary_to_css,
        ),
    }
}

fn list_to_css<T>(list: &[T], dest: &mut String, mut item: impl FnMut(&T, &mut String)) {
    for (i, value) in list.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        item(value, dest);
    }
}

/// `fill` or `stroke`, its colors resolved (Blink's
/// `ComputedStyleUtils::ValueForSVGPaint`).
fn paint_to_css(style: &ComputedStyle, paint: &SvgPaint, dest: &mut String) {
    match paint {
        SvgPaint::None => dest.push_str("none"),
        SvgPaint::Color(color) => style_color_to_css(style, *color, dest),
        SvgPaint::Url(url, fallback) => {
            url_to_css(url, dest);
            match fallback {
                SvgPaintFallback::Absent => {}
                SvgPaintFallback::None => dest.push_str(" none"),
                SvgPaintFallback::Color(color) => {
                    dest.push(' ');
                    style_color_to_css(style, *color, dest);
                }
            }
        }
    }
}

/// `box-shadow` as Chrome resolves it (`ComputedStyleUtils::ValueForShadowList`):
/// each shadow's color, its four lengths, then `inset`.
/// `box-shadow` (with the spread), or `text-shadow`.
fn shadows_to_css(
    style: &ComputedStyle,
    shadows: &[computed_style::BoxShadow],
    spread: bool,
    dest: &mut String,
) {
    if shadows.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, shadow) in shadows.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        style_color_to_css(style, shadow.color, dest);
        let lengths = [shadow.x, shadow.y, shadow.blur, shadow.spread];
        for length in &lengths[..if spread { 4 } else { 3 }] {
            dest.push(' ');
            px_to_css(*length, dest);
        }
        if shadow.inset {
            dest.push_str(" inset");
        }
    }
}

/// One radius when both are the same, else both.
fn corner_radius_to_css(radius: computed_style::CornerRadius, dest: &mut String) {
    length_percentage(radius.x, dest);
    if radius.y != radius.x {
        dest.push(' ');
        length_percentage(radius.y, dest);
    }
}

fn none_or(value: LengthPercentageOrNone, dest: &mut String) {
    match value {
        LengthPercentageOrNone::None => dest.push_str("none"),
        LengthPercentageOrNone::LengthPercentage(value) => length_percentage(value, dest),
    }
}
