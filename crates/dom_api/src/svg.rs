//! SVG's `SVGElement` (Blink's `core/svg/svg_element.cc`): the members that
//! need no layout, and `SVGAnimatedString`. The rest of an SVG element's
//! members are `HTMLElement`'s too, and live with them (`focus`,
//! `dataset`, `css_style`).

use dom::{DomString, NodeRef, SvgAnimatedString, names};
use elise::Gc;

use crate::{ApiCx, ApiError, DomStr};

/// Whether `node` is an SVG element named `local`.
fn is_svg<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, local: names::NameId) -> bool {
    cx.qualified_name(node)
        .is_some_and(|name| name.is(names::SVG_NAMESPACE, local))
}

/// An outermost `<svg>`: one whose parent isn't an SVG element (or is a
/// `foreignObject`, whose children are another document's content).
fn is_outermost_svg<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    is_svg(cx, node, names::local::SVG)
        && cx.parent(node).is_none_or(|parent| {
            cx.qualified_name(parent)
                .is_none_or(|name| name.namespace() != names::SVG_NAMESPACE)
                || is_svg(cx, parent, names::local::FOREIGN_OBJECT)
        })
}

/// The nearest ancestor `matches` passes.
fn nearest_ancestor<'gc>(
    cx: &ApiCx<'_, 'gc>,
    node: NodeRef<'gc>,
    matches: impl Fn(NodeRef<'gc>) -> bool,
) -> Option<NodeRef<'gc>> {
    std::iter::successors(cx.parent(node), |&ancestor| cx.parent(ancestor)).find(|&a| matches(a))
}

/// `ownerSVGElement`: the nearest `<svg>` ancestor, `null` for an outermost
/// `<svg>` (even one inside another's `foreignObject`, as Chrome) and an
/// element in none.
pub(crate) fn owner_svg_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    if is_outermost_svg(cx, this) {
        return Ok(None);
    }
    Ok(nearest_ancestor(cx, this, |ancestor| {
        is_svg(cx, ancestor, names::local::SVG)
    }))
}

/// `viewportElement`: the nearest ancestor that establishes a viewport, an
/// `<svg>`, `<symbol>` or `<image>` (Blink's `SVGElement::viewportElement`),
/// through `foreignObject` too, as Chrome; `null` when there is none.
pub(crate) fn viewport_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(nearest_ancestor(cx, this, |ancestor| {
        [names::local::SVG, names::local::SYMBOL, names::local::IMAGE]
            .into_iter()
            .any(|local| is_svg(cx, ancestor, local))
    }))
}

/// `className`: the same `SVGAnimatedString` over the `class` attribute
/// each time.
pub(crate) fn class_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, SvgAnimatedString>, ApiError> {
    Ok(cx.svg_class_name(this))
}

/// `baseVal`: its attribute's value, the empty string without one.
pub(crate) fn base_val<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgAnimatedString>,
) -> Result<DomStr<'gc>, ApiError> {
    let (element, attribute) = cx.svg_animated_string_attribute(this);
    let value = cx
        .attribute_value_by_id(element, attribute.local())
        .map_or_else(|| DomString::from(""), |value| value.to_owned());
    Ok(DomStr::Owned(value))
}

/// Setting `baseVal` sets the attribute.
pub(crate) fn set_base_val<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgAnimatedString>,
    value: &str,
) -> Result<(), ApiError> {
    let (element, attribute) = cx.svg_animated_string_attribute(this);
    let name: DomString = names::string(attribute.local());
    cx.set_attribute(element, &name.to_str(), value);
    Ok(())
}

/// `animVal`: `baseVal`, as there are no animations.
pub(crate) fn anim_val<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, SvgAnimatedString>,
) -> Result<DomStr<'gc>, ApiError> {
    base_val(cx, this)
}
