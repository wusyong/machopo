//! `getComputedStyle` (https://drafts.csswg.org/cssom/#dom-window-getcomputedstyle;
//! Blink's `LocalDOMWindow::getComputedStyle` and
//! `CSSComputedStyleDeclaration`): the read-only, live
//! `CSSStyleDeclaration` over an element's computed style
//! (`css_style.rs`'s members, for a declaration whose owner is
//! `DeclarationOwner::Computed`).
//!
//! Reading a value brings the element's document up to date first: its
//! style for most properties, and its layout for those whose resolved
//! value is the used value (`width` and its kin, Blink's
//! `IsLayoutDependent`), which come from the element's box. Without a box
//! (`display: none`, a document no frame shows) the computed value
//! stands, as Chrome's does.

use std::sync::Arc;

use computed_style::{ComputedStyle, PseudoId};
use dom::{CssStyleDeclaration, Cx, NodeRef, StyleTarget, Window};
use elise::Gc;
use gfx::ElementBox;

use crate::exception::{DomException, ExceptionName};
use crate::layout::LentLayout;
use crate::{ApiCx, ApiError, FrameLayout};

/// The pseudo-elements a selector may name. A declaration asked for one
/// machopo doesn't have has no style at all, as Blink's has when it has no
/// style for one; anything else is a `SyntaxError`, as the spec says.
const PSEUDO_ELEMENTS: [&str; 10] = [
    "before",
    "after",
    "marker",
    "first-line",
    "first-letter",
    "backdrop",
    "placeholder",
    "selection",
    "file-selector-button",
    "target-text",
];

/// `window.getComputedStyle(element, pseudoElt)`: a new declaration each
/// call (`[NewObject]`), live. The window is the one the call came
/// through; the element's own document is what gets updated.
pub(crate) fn get_computed_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Window>,
    element: NodeRef<'gc>,
    pseudo: Option<&str>,
) -> Result<Gc<'gc, CssStyleDeclaration>, ApiError> {
    let target = match pseudo {
        // Not starting with a colon: the element's own (CSSOM, as Chrome).
        None => StyleTarget::Element,
        Some(text) if !text.starts_with(':') => StyleTarget::Element,
        Some(text) => {
            let (name, single_colon) = match text.strip_prefix("::") {
                Some(name) => (name, false),
                None => (&text[1..], true),
            };
            let name = name.to_ascii_lowercase();
            // Only CSS 2's pseudo-elements take one colon: `:marker` is a
            // known name, but no pseudo-element.
            let css2 = matches!(
                name.as_str(),
                "before" | "after" | "first-line" | "first-letter"
            );
            match name.as_str() {
                _ if single_colon && !css2 && PSEUDO_ELEMENTS.contains(&name.as_str()) => {
                    StyleTarget::OtherPseudo
                }
                "before" => StyleTarget::Pseudo(PseudoId::Before),
                "after" => StyleTarget::Pseudo(PseudoId::After),
                "marker" => StyleTarget::Pseudo(PseudoId::Marker),
                "placeholder" => StyleTarget::Placeholder,
                "selection" => StyleTarget::Selection,
                other if PSEUDO_ELEMENTS.contains(&other) => StyleTarget::OtherPseudo,
                _ => {
                    return Err(ApiError::Dom(DomException {
                        name: ExceptionName::Syntax,
                        message: format!("'{text}' is not a pseudo-element selector"),
                    }));
                }
            }
        }
    };
    Ok(cx.new_computed_style_declaration(element, target))
}

/// What a computed declaration's setters throw: it is read only.
pub(crate) fn read_only() -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::NoModificationAllowed,
        message: "These styles are computed, and are therefore read-only.".to_owned(),
    })
}

/// The names `length` and `item()` list: every longhand machopo supports
/// (Blink's `ComputableProperties`), then the element's custom properties,
/// sorted (Chrome's order is its hash table's). None at all without a
/// style to show: a declaration for a pseudo-element machopo doesn't have, or
/// for an element no document renders (CSSOM: it has no declarations, and
/// Chrome lists none).
pub(crate) fn property_names<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    target: StyleTarget,
) -> Vec<String> {
    let Some(style) = style_of_target(cx, element, target) else {
        return Vec::new();
    };
    let mut custom = style_resolver::custom_property_names(&style);
    custom.sort_unstable();
    style_resolver::computable_properties()
        .into_iter()
        .map(str::to_owned)
        .chain(custom)
        .collect()
}

/// Each property `getComputedStyle(element)` lists, with its value, as
/// it reads them (`property_names`, `property_value`): DevTools' `CSS`
/// agent's `getComputedStyleForNode` (Blink's reads its
/// `CSSComputedStyleDeclaration`), the host lending its layout. Empty for
/// an element no frame of `layout`'s renders.
pub fn computed_style_for_devtools<'gc>(
    cx: &Cx<'_, 'gc>,
    layout: &mut dyn FrameLayout,
    element: NodeRef<'gc>,
) -> Vec<(String, String)> {
    let layout = LentLayout::new(layout);
    let cx = ApiCx {
        dom: cx,
        layout: Some(&layout),
    };
    property_names(&cx, element, StyleTarget::Element)
        .into_iter()
        .map(|name| {
            let value = property_value(&cx, element, StyleTarget::Element, &name);
            (name, value)
        })
        .collect()
}

/// The values `getComputedStyle(element)` reads of `properties`, in order,
/// the host lending its layout: what DevTools' overlay shows of an
/// element's style in its label (Blink's `BuildStyleInfo`).
pub fn computed_values_for_devtools<'gc>(
    cx: &Cx<'_, 'gc>,
    layout: &mut dyn FrameLayout,
    element: NodeRef<'gc>,
    properties: &[&str],
) -> Vec<String> {
    let layout = LentLayout::new(layout);
    let cx = ApiCx {
        dom: cx,
        layout: Some(&layout),
    };
    properties
        .iter()
        .map(|property| property_value(&cx, element, StyleTarget::Element, property))
        .collect()
}

/// `property`'s resolved value, the empty string if it isn't a property
/// machopo supports or the element has no style.
pub(crate) fn property_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    target: StyleTarget,
    property: &str,
) -> String {
    // The style first, as Blink does (`UpdateStyleAndLayoutTreeForElement`,
    // then `EnsureComputedStyle`): nothing to read without one.
    let Some(style) = style_of_target(cx, element, target) else {
        return String::new();
    };
    // A shorthand Chrome writes from the style itself.
    if let Some(value) = style_resolver::shorthand_from_style(&style, property) {
        return value;
    }
    // Another shorthand: each longhand resolved, then written back as it
    // (Blink's `StylePropertySerializer` over the computed style).
    if let Some(longhands) = style_resolver::shorthand_longhands(property) {
        let values: Vec<(&str, String)> = longhands
            .into_iter()
            .map(|name| (name, property_value(cx, element, target, name)))
            .collect();
        return style_resolver::shorthand_value(property, &values).unwrap_or_default();
    }
    // A flow-relative property resolves as its physical one (Blink's
    // `ResolveDirectionAwareProperty`).
    let property = style_resolver::physical_property(property).unwrap_or(property);
    // A pseudo-element's are its computed values: its box isn't looked up
    // (Chrome resolves them from it, as an element's).
    if style_resolver::is_layout_dependent(property) && target == StyleTarget::Element {
        let id = cx.node_id(element);
        let element_box = cx
            .with_layout(element, |layout| layout.element_box(id))
            .flatten();
        if let Some(element_box) = element_box
            && let Some(value) = resolved_from_box(property, &element_box, &style)
        {
            return value;
        }
        // No box (`display: none`): the computed value. For `transform`
        // that is `none`, as it is in Chrome, whose resolved value needs
        // the box; but for an SVG element, which never has a CSS box, its
        // matrix (Chrome's reference box is then the SVG viewport, which
        // machopo doesn't hand out yet: percentages are of nothing).
        if element_box.is_none() && property.eq_ignore_ascii_case("transform") {
            let svg = cx
                .qualified_name(element)
                .is_some_and(|name| name.namespace() == dom::names::SVG_NAMESPACE);
            if svg && style.display != computed_style::Display::None {
                return style_resolver::resolved_transform(&style, gfx::SizeF::default());
            }
            return "none".to_owned();
        }
    }
    style_resolver::computed_value(&style, property).unwrap_or_default()
}

/// A layout-dependent property's resolved value from the element's box:
/// `transform`'s matrix against the border box, `transform-origin` of its
/// reference box (`transform-box`), and the used values `used_value`
/// gives. `None` where the computed value stands (an inline element's).
fn resolved_from_box(
    property: &str,
    element_box: &ElementBox,
    style: &ComputedStyle,
) -> Option<String> {
    if property.eq_ignore_ascii_case("transform") {
        return Some(style_resolver::resolved_transform(
            style,
            element_box.border_box.size(),
        ));
    }
    // Of its reference box: the content box for `content-box` and
    // `fill-box`, else the border box. An inline element, which no
    // transform applies to, has its computed value.
    if property.eq_ignore_ascii_case("transform-origin") {
        let block = element_box.block?;
        let mut size = element_box.border_box.size();
        if matches!(
            style.rare.transform_box,
            computed_style::TransformBox::ContentBox | computed_style::TransformBox::FillBox
        ) {
            let [top, right, bottom, left] =
                [0, 1, 2, 3].map(|i| block.border[i] + block.padding[i]);
            size.width = (size.width - left - right).max(0.0);
            size.height = (size.height - top - bottom).max(0.0);
        }
        return Some(style_resolver::resolved_transform_origin(style, size));
    }
    used_value(property, element_box, style)
}

/// A layout-dependent property's used value from the element's box: the
/// content box for `width` and `height`, the box's own padding and
/// margins for the others, and a positioned box's insets. `None` for an
/// inline element, which has no content box of its own (Chrome resolves
/// those to the computed value), and for a property this doesn't resolve.
fn used_value(property: &str, element_box: &ElementBox, style: &ComputedStyle) -> Option<String> {
    // Property names are ASCII case-insensitive, as they are everywhere
    // else (`style_resolver::computed_value`).
    let property = property.to_ascii_lowercase();
    let block = element_box.block?;
    let [top, right, bottom, left] = match property.as_str() {
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => block.padding,
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => block.margin,
        // A static box's insets stay `auto`, their computed value, as in
        // Chrome; a positioned one's are where layout put it. A sticky
        // one's `auto` stays `auto` too: it says the box doesn't stick on
        // that side (Chrome's `min_inset_for_get_computed_style`).
        "top" | "right" | "bottom" | "left"
            if style.position == computed_style::Position::Sticky
                && *inset_of(style, &property) == computed_style::LengthPercentageOrAuto::Auto =>
        {
            return None;
        }
        "top" | "right" | "bottom" | "left" if style.is_positioned() => block.inset,
        "width" | "height" => {
            let [bt, br, bb, bl] = block.border;
            let [pt, pr, pb, pl] = block.padding;
            let size = element_box.border_box;
            let px = if property == "width" {
                size.width - bl - br - pl - pr
            } else {
                size.height - bt - bb - pt - pb
            };
            return Some(px_to_css(px));
        }
        _ => return None,
    };
    let px = match property.rsplit('-').next()? {
        "top" => top,
        "right" => right,
        "bottom" => bottom,
        _ => left,
    };
    Some(px_to_css(px))
}

/// `style`'s inset that `property` (one of `top`, `right`, `bottom`,
/// `left`) is.
fn inset_of<'a>(
    style: &'a ComputedStyle,
    property: &str,
) -> &'a computed_style::LengthPercentageOrAuto {
    match property {
        "top" => &style.inset.top,
        "right" => &style.inset.right,
        "bottom" => &style.inset.bottom,
        _ => &style.inset.left,
    }
}

fn px_to_css(px: f32) -> String {
    let mut out = style_resolver::write_css_number(f64::from(px));
    out.push_str("px");
    out
}

/// The style a declaration for `target` shows: the element's, or one of
/// its pseudo-elements'. A pseudo-element no rule matches has the style
/// cascading nothing gives it (Blink's `EnsureComputedStyle` computes it
/// when asked); one machopo doesn't have, none.
fn style_of_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    target: StyleTarget,
) -> Option<Arc<ComputedStyle>> {
    if target == StyleTarget::OtherPseudo {
        return None;
    }
    cx.with_style(element, |layout| {
        let style = layout.computed_style(cx, element)?;
        Some(match target {
            StyleTarget::Element | StyleTarget::OtherPseudo => style,
            StyleTarget::Pseudo(pseudo) => {
                layout.pseudo_style(cx, element, pseudo).unwrap_or_else(|| {
                    Arc::new(style_resolver::pseudo_style_without_rules(&style, pseudo))
                })
            }
            // The placeholder element's, or (a control without one) what
            // cascading nothing gives it, as Chrome.
            StyleTarget::Placeholder => cx
                .placeholder_element(element)
                .and_then(|placeholder| layout.computed_style(cx, placeholder))
                .unwrap_or_else(|| Arc::new(ComputedStyle::inherit(&style))),
            // The element's style with its highlight's properties (the
            // root's initial ones while no `::selection` rule matches it or
            // an ancestor: a transparent background, the element's color).
            StyleTarget::Selection => {
                let highlight = style
                    .rare_inherited
                    .highlight
                    .as_deref()
                    .cloned()
                    .unwrap_or_else(computed_style::HighlightStyle::initial);
                let mut selection = (*style).clone();
                highlight.apply_to(&mut selection);
                if let Some(variables) = &style.rare.highlight_variables {
                    selection.variables = variables.clone();
                }
                Arc::new(selection)
            }
        })
    })
    .flatten()
}
