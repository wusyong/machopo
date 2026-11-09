//! CSSOM View's members that read the layout: `Element`'s rects,
//! `client*` and `scroll*`, `HTMLElement`'s `offset*` and `offsetParent`,
//! `Document`'s `elementFromPoint` and `scrollingElement`, and `Window`'s
//! viewport, as Blink's `Element` computes them from its `LayoutBox`
//! (`element.cc`, `LayoutBoxModelObject::OffsetPoint`); and Geometry
//! Interfaces' `DOMRectReadOnly`, `DOMRect` and `DOMRectList`, what the
//! rects are given as. Each brings the style and layout of the frame up to
//! date first, through the layout the engine lends (`ApiCx::with_layout`,
//! Blink's `Document::UpdateStyleAndLayoutForNode`), all it reads in that
//! one lending. Lengths are whole CSS px, rounded as Blink's
//! `LayoutUnit::Round` (half away from zero); scroll offsets aren't.

use base::numerics::{clamp_round, f32_from_f64, saturated_cast};
use computed_style::ComputedStyle;
use dom::names::{self, NameId, local};
use dom::{DomRect, DomRectList, NodeKind, NodeRef, QuirksMode, Window};
use elise::Gc;
use gfx::{ElementBox, PointF, RectF};

use crate::interfaces::DOMRectInit;
use crate::layout::{FrameLayout, Hit};
use crate::typed::DictValue;
use crate::{ApiCx, ApiError};

/// `new DOMRectReadOnly(x, y, width, height)`, each 0 if not given.
pub(crate) fn construct_read_only_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<Gc<'gc, DomRect>, ApiError> {
    Ok(cx.new_rect_of(given([x, y, width, height]), true))
}

/// `new DOMRect(x, y, width, height)`, each 0 if not given.
pub(crate) fn construct_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<Gc<'gc, DomRect>, ApiError> {
    Ok(cx.new_rect_of(given([x, y, width, height]), false))
}

/// `DOMRectReadOnly.fromRect(other)`.
pub(crate) fn read_only_rect_from_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    other: &DOMRectInit,
) -> Result<Gc<'gc, DomRect>, ApiError> {
    Ok(cx.new_rect_of(init_values(other), true))
}

/// `DOMRect.fromRect(other)`.
pub(crate) fn rect_from_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    other: &DOMRectInit,
) -> Result<Gc<'gc, DomRect>, ApiError> {
    Ok(cx.new_rect_of(init_values(other), false))
}

fn given(values: [Option<f64>; 4]) -> [f64; 4] {
    values.map(|value| value.unwrap_or(0.0))
}

fn init_values(init: &DOMRectInit) -> [f64; 4] {
    given([init.x, init.y, init.width, init.height])
}

/// `toJSON()`, WebIDL's default one: an object with each attribute's
/// value, in the interface's order (Blink's
/// `DOMRectReadOnly::toJSONForBinding`).
pub(crate) fn to_json<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRect>,
) -> Result<Vec<(&'static str, DictValue)>, ApiError> {
    type Getter = for<'a, 'g> fn(&ApiCx<'a, 'g>, Gc<'g, DomRect>) -> Result<f64, ApiError>;
    let getters: [(&'static str, Getter); 8] = [
        ("x", x),
        ("y", y),
        ("width", width),
        ("height", height),
        ("top", top),
        ("right", right),
        ("bottom", bottom),
        ("left", left),
    ];
    getters
        .into_iter()
        .map(|(name, get)| Ok((name, DictValue::Number(get(cx, this)?))))
        .collect()
}

/// `x`, `y`, `width` and `height` of a `DOMRectReadOnly` (or a `DOMRect`).
pub(crate) fn x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    Ok(cx.rect(this)[0])
}

pub(crate) fn y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    Ok(cx.rect(this)[1])
}

pub(crate) fn width<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    Ok(cx.rect(this)[2])
}

pub(crate) fn height<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    Ok(cx.rect(this)[3])
}

/// `top`: the smaller of `y` and `y + height` (a height may be negative).
pub(crate) fn top<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    let [_, y, _, height] = cx.rect(this);
    Ok(nan_safe_min(y, y + height))
}

pub(crate) fn right<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    let [x, _, width, _] = cx.rect(this);
    Ok(nan_safe_max(x, x + width))
}

pub(crate) fn bottom<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    let [_, y, _, height] = cx.rect(this);
    Ok(nan_safe_max(y, y + height))
}

pub(crate) fn left<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomRect>) -> Result<f64, ApiError> {
    let [x, _, width, _] = cx.rect(this);
    Ok(nan_safe_min(x, x + width))
}

/// The spec's `min`, NaN if either is (Blink's `NanSafeMin`); `f64::min`
/// would take the other.
fn nan_safe_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

fn nan_safe_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// A `DOMRect`'s own `x`, `y`, `width` and `height` setters.
pub(crate) fn set_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRect>,
    value: f64,
) -> Result<(), ApiError> {
    cx.set_rect_value(this, 0, value);
    Ok(())
}

pub(crate) fn set_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRect>,
    value: f64,
) -> Result<(), ApiError> {
    cx.set_rect_value(this, 1, value);
    Ok(())
}

pub(crate) fn set_width<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRect>,
    value: f64,
) -> Result<(), ApiError> {
    cx.set_rect_value(this, 2, value);
    Ok(())
}

pub(crate) fn set_height<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRect>,
    value: f64,
) -> Result<(), ApiError> {
    cx.set_rect_value(this, 3, value);
    Ok(())
}

pub(crate) fn rect_list_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRectList>,
) -> Result<u32, ApiError> {
    Ok(cx.rect_list_length(this))
}

pub(crate) fn rect_list_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomRectList>,
    index: u32,
) -> Result<Option<Gc<'gc, DomRect>>, ApiError> {
    Ok(cx.rect_list_item(this, index))
}

/// `getClientRects()`: a new list each call, empty if the element has no
/// box (`display: none`, or not in the document a frame shows).
pub(crate) fn get_client_rects<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomRectList>, ApiError> {
    let rects = client_rects(cx, this);
    Ok(cx.new_rect_list(&rects))
}

/// `getBoundingClientRect()`: a new rect each call, around the element's
/// boxes (CSSOM View's: the union of those that aren't empty, else the
/// first); all zero without a box.
pub(crate) fn get_bounding_client_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomRect>, ApiError> {
    Ok(cx.new_rect(bounding_client_rect(cx, this)))
}

/// `getClientRects()`'s rects, laid out first.
pub(crate) fn client_rects_of<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> Vec<RectF> {
    client_rects(cx, element)
}

/// `getBoundingClientRect()`'s rect, laid out first.
pub(crate) fn bounding_client_rect<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> RectF {
    bounding_rect(&client_rects(cx, element))
}

/// The union of `rects` that aren't empty, else the first; all zero for
/// none (CSSOM View's bounding rect).
pub(crate) fn bounding_rect(rects: &[RectF]) -> RectF {
    rects
        .iter()
        .filter(|rect| rect.width != 0.0 && rect.height != 0.0)
        .copied()
        .reduce(|a, b| a.union(&b))
        .or_else(|| rects.first().copied())
        .unwrap_or(RectF::new(0.0, 0.0, 0.0, 0.0))
}

/// Its boxes' border boxes, or an SVG element's one rect, which isn't a
/// CSS box (its bounding box where it shows).
fn client_rects<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> Vec<RectF> {
    cx.with_layout(element, |layout| client_rects_in(cx, layout, element))
        .unwrap_or_default()
}

/// `client_rects` in a layout up to date (a range's elements').
pub(crate) fn client_rects_in<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &mut dyn crate::FrameLayout,
    element: NodeRef<'gc>,
) -> Vec<RectF> {
    let rects = layout.client_rects(cx.node_id(element));
    let svg = cx
        .qualified_name(element)
        .is_some_and(|name| name.namespace() == names::SVG_NAMESPACE);
    if !rects.is_empty() || !svg {
        return rects;
    }
    layout
        .svg_geometry(cx, element)
        .client_rect
        .into_iter()
        .collect()
}

/// `element`'s box as just laid out.
fn box_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &dyn FrameLayout,
    element: NodeRef<'gc>,
) -> Option<ElementBox> {
    layout.element_box(cx.node_id(element))
}

fn round(length: f32) -> i32 {
    clamp_round(length)
}

/// Whether `node` is an HTML element of one of `locals`.
fn is_html<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>, locals: &[NameId]) -> bool {
    cx.element_name(node)
        .is_some_and(|name| name.namespace == names::HTML_NAMESPACE && locals.contains(&name.local))
}

pub(crate) fn document_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    std::iter::successors(cx.first_child(document), |&child| cx.next_sibling(child))
        .find(|&child| cx.kind(child) == NodeKind::Element)
}

/// HTML's "the body element": the root `html` element's first `body` or
/// `frameset` child.
pub(crate) fn body<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let root = document_element(cx, document).filter(|&root| is_html(cx, root, &[local::HTML]))?;
    std::iter::successors(cx.first_child(root), |&child| cx.next_sibling(child))
        .find(|&child| is_html(cx, child, &[local::BODY, local::FRAMESET]))
}

fn in_quirks_mode<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> bool {
    cx.quirks_mode(document) == QuirksMode::Quirks
}

/// Whether `element`'s computed style says so (its `position`), as just
/// styled.
fn style_says<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &dyn FrameLayout,
    element: NodeRef<'gc>,
    says: impl Fn(&ComputedStyle) -> bool,
) -> bool {
    layout
        .computed_style(cx, element)
        .is_some_and(|style| says(&style))
}

/// Whether `element` stands for the viewport in `client*` (Blink's
/// `IsViewportScrollElement`): the root element, or in quirks mode the
/// body element.
fn is_viewport_element<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    let document = cx.document_of(element);
    let viewport_element = if in_quirks_mode(cx, document) {
        body(cx, document)
    } else {
        document_element(cx, document)
    };
    viewport_element.is_some_and(|viewport_element| Gc::ptr_eq(viewport_element, element))
}

/// CSSOM View's "scrolling element": the root element; in quirks mode the
/// body element, unless it is potentially scrollable (a scroll container
/// of its own: its `overflow` didn't go to the viewport). `layout` is the
/// laid-out frame's, if `document` has one.
fn scrolling_element_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: Option<&dyn FrameLayout>,
    document: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    if !in_quirks_mode(cx, document) {
        return document_element(cx, document);
    }
    let body = body(cx, document)?;
    let scrolls = layout
        .and_then(|layout| box_of(cx, layout, body)?.block)
        .is_some_and(|block| block.scroll.is_some());
    (!scrolls).then_some(body)
}

/// `document.scrollingElement`: laid out first only in quirks mode, where
/// the body element's box decides.
pub(crate) fn scrolling_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    if !in_quirks_mode(cx, this) {
        return Ok(document_element(cx, this));
    }
    Ok(cx
        .with_layout(this, |layout| {
            scrolling_element_of(cx, Some(&*layout), this)
        })
        .unwrap_or_else(|| scrolling_element_of(cx, None, this)))
}

/// `offsetParent` (Blink's `LayoutObject::OffsetParent`): the nearest
/// ancestor that is positioned, or is the body element, a `table`, `td`
/// or `th`; `null` for an element without a box, the root element, the
/// body element and a fixed element (whose offsets are the viewport's).
pub(crate) fn offset_parent<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx
        .with_layout(this, |layout| offset_parent_of(cx, layout, this))
        .flatten())
}

fn offset_parent_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &dyn FrameLayout,
    element: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    box_of(cx, layout, element)?;
    // The root element has no ancestor element to find: it needs no check
    // of its own. A fixed element is laid out against the viewport, so its
    // offsets are the document origin's: there is no parent to be
    // relative to.
    if is_html(cx, element, &[local::BODY])
        || style_says(cx, layout, element, ComputedStyle::is_fixed)
    {
        return None;
    }
    // In the flat tree, as the boxes are, past those of a shadow tree the
    // element isn't in, open or closed (what retargeting against it
    // changes: Chrome's, and WPT's `offsetParent-across-shadow-boundaries`).
    let flat_parent = |node| {
        cx.flat_tree_parent(node)
            .filter(|&parent| cx.kind(parent) == NodeKind::Element)
    };
    std::iter::successors(flat_parent(element), |&node| flat_parent(node)).find(|&node| {
        elise::Gc::ptr_eq(cx.retarget(node, Some(element)), node)
            && (style_says(cx, layout, node, ComputedStyle::is_positioned)
                || is_html(cx, node, &[local::BODY, local::TABLE, local::TD, local::TH]))
    })
}

/// Where `element` starts relative to its `offsetParent`'s padding box
/// (Blink's `AdjustedPositionRelativeTo`): the boxes' offsets in between
/// add up, transforms and scrolling aside; the body element's is relative
/// to the root element's border box, and one without an `offsetParent` to
/// the document's origin.
fn offset_point<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &dyn FrameLayout,
    element: NodeRef<'gc>,
) -> PointF {
    let origin = PointF::new(0.0, 0.0);
    let Some(own) = box_of(cx, layout, element) else {
        return origin;
    };
    if is_html(cx, element, &[local::BODY]) {
        return origin;
    }
    let start = own.start;
    let Some(parent) = offset_parent_of(cx, layout, element) else {
        return start;
    };
    let reference = if is_html(cx, parent, &[local::BODY]) {
        document_element(cx, cx.document_of(element))
            .and_then(|root| box_of(cx, layout, root))
            .map_or(origin, |root| {
                PointF::new(root.border_box.x, root.border_box.y)
            })
    } else {
        match box_of(cx, layout, parent) {
            Some(ElementBox {
                border_box,
                block: Some(block),
                ..
            }) => PointF::new(
                border_box.x + block.border[3],
                border_box.y + block.border[0],
            ),
            // An inline one: from its first box on a line.
            Some(inline) => inline.start,
            None => origin,
        }
    };
    PointF::new(start.x - reference.x, start.y - reference.y)
}

/// `f` of `element`'s box, the frame laid out first; none without a box.
fn with_box<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    f: impl FnOnce(ElementBox) -> R,
) -> Option<R> {
    cx.with_layout(element, |layout| box_of(cx, layout, element).map(f))
        .flatten()
}

pub(crate) fn offset_left<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(cx
        .with_layout(this, |layout| round(offset_point(cx, layout, this).x))
        .unwrap_or(0))
}

pub(crate) fn offset_top<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(cx
        .with_layout(this, |layout| round(offset_point(cx, layout, this).y))
        .unwrap_or(0))
}

/// `offsetWidth`: the border box's, or an inline element's lines'.
pub(crate) fn offset_width<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(with_box(cx, this, |found| round(found.border_box.width)).unwrap_or(0))
}

pub(crate) fn offset_height<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(with_box(cx, this, |found| round(found.border_box.height)).unwrap_or(0))
}

/// `clientTop`: the top border's width; 0 for an inline element.
pub(crate) fn client_top<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(with_box(cx, this, |found| {
        found.block.map_or(0, |block| round(block.border[0]))
    })
    .unwrap_or(0))
}

pub(crate) fn client_left<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(with_box(cx, this, |found| {
        found.block.map_or(0, |block| round(block.border[3]))
    })
    .unwrap_or(0))
}

/// `clientWidth`: the padding box's (no scrollbars); the viewport's for
/// the root element (the body element in quirks mode); 0 for an inline
/// element.
pub(crate) fn client_width<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    let viewport_element = is_viewport_element(cx, this);
    Ok(cx
        .with_layout(this, |layout| {
            if viewport_element {
                return saturated_cast(layout.viewport().size.width);
            }
            match box_of(cx, layout, this) {
                Some(ElementBox {
                    border_box,
                    block: Some(block),
                    ..
                }) => round(border_box.width - block.border[1] - block.border[3]),
                _ => 0,
            }
        })
        .unwrap_or(0))
}

pub(crate) fn client_height<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    let viewport_element = is_viewport_element(cx, this);
    Ok(cx
        .with_layout(this, |layout| {
            if viewport_element {
                return saturated_cast(layout.viewport().size.height);
            }
            match box_of(cx, layout, this) {
                Some(ElementBox {
                    border_box,
                    block: Some(block),
                    ..
                }) => round(border_box.height - block.border[0] - block.border[2]),
                _ => 0,
            }
        })
        .unwrap_or(0))
}

/// Whether `element` is its document's scrolling element, whose `scroll*`
/// are the viewport's.
pub(crate) fn is_scrolling_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    layout: &dyn FrameLayout,
    element: NodeRef<'gc>,
) -> bool {
    scrolling_element_of(cx, Some(layout), cx.document_of(element))
        .is_some_and(|scrolling| Gc::ptr_eq(scrolling, element))
}

/// How far `element` is scrolled: the viewport's for the scrolling
/// element; 0 for one that isn't a scroll container.
fn scroll_offset<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> PointF {
    cx.with_layout(element, |layout| {
        if is_scrolling_element(cx, layout, element) {
            return Some(layout.viewport().scroll);
        }
        box_of(cx, layout, element)?.block?.scroll
    })
    .flatten()
    .unwrap_or(PointF::new(0.0, 0.0))
}

pub(crate) fn scroll_top<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<f64, ApiError> {
    Ok(f64::from(scroll_offset(cx, this).y))
}

pub(crate) fn scroll_left<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<f64, ApiError> {
    Ok(f64::from(scroll_offset(cx, this).x))
}

/// `scrollWidth`: the scrollable overflow's, at least the padding box's;
/// the document's for the scrolling element; 0 for an inline element.
pub(crate) fn scroll_width<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(cx
        .with_layout(this, |layout| {
            if is_scrolling_element(cx, layout, this) {
                return round(layout.viewport().content_size.width);
            }
            box_of(cx, layout, this)
                .and_then(|found| found.block)
                .map_or(0, |block| round(block.scroll_size.width))
        })
        .unwrap_or(0))
}

pub(crate) fn scroll_height<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<i32, ApiError> {
    Ok(cx
        .with_layout(this, |layout| {
            if is_scrolling_element(cx, layout, this) {
                return round(layout.viewport().content_size.height);
            }
            box_of(cx, layout, this)
                .and_then(|found| found.block)
                .map_or(0, |block| round(block.scroll_size.height))
        })
        .unwrap_or(0))
}

/// `elementFromPoint(x, y)` of a document: the topmost element there, its
/// frame laid out and pre-painted first (Blink's
/// `TreeScope::ElementFromPoint`): the root element where only the canvas
/// is; `null` outside the viewport, or without a frame.
pub(crate) fn element_from_point<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    x: f64,
    y: f64,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(
        element_at(cx, this, PointF::new(f32_from_f64(x), f32_from_f64(y)))
            .map(|element| cx.retarget(element, Some(this))),
    )
}

/// The topmost element at `point` (CSS px of the viewport) of `document`'s
/// (`elementFromPoint`, WebDriver's): what `deepest_element_at` finds, or
/// for a run of text, the text's parent element (Blink's
/// `TreeScope::HitTestPoint` takes the text's parent, not the slot it is
/// shown in; a shadow root's own text is the host's, the element hit); for
/// one in a
/// child frame, the iframe here it is in (Blink's hit test doesn't go into
/// child frames for these); the root element for one that isn't in this
/// frame (an element of a parent's painted over its iframe).
pub(crate) fn element_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    point: PointF,
) -> Option<NodeRef<'gc>> {
    let hit = hit_at(cx, document, point, true)?;
    let text_parent = hit
        .text
        .and_then(|text| cx.parent(cx.node_by_id(text)?))
        .filter(|&parent| cx.kind(parent) == NodeKind::Element);
    let mut node = text_parent
        .or_else(|| hit.element.and_then(|element| cx.node_by_id(element)))
        .or_else(|| document_element(cx, document))?;
    // A frame tree is fewer than u16::MAX deep.
    for _ in 0..u16::MAX {
        if Gc::ptr_eq(cx.document_of(node), document) {
            return Some(node);
        }
        match cx.frame_owner(cx.document_of(node)) {
            Some(owner) => node = owner,
            None => break,
        }
    }
    document_element(cx, document)
}

/// The topmost element at `point` (CSS px of the viewport) in `document`'s
/// frame, laid out and pre-painted first, through the child frames: the
/// root element where only the canvas is; none outside the viewport or
/// without a frame.
pub(crate) fn deepest_element_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    point: PointF,
) -> Option<NodeRef<'gc>> {
    deepest_hit_at(cx, document, point, true).0
}

/// `deepest_element_at`, and whether what it hit is a run of the
/// element's text (`FrameLayout::hit_test`): what the mouse is over, and
/// its cursor's I-beam. Outside the viewport only if `in_viewport_only`
/// isn't set: the mouse's events still hit what is laid out there while a
/// button pressed in the frame drags out of it.
pub(crate) fn deepest_hit_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    point: PointF,
    in_viewport_only: bool,
) -> (Option<NodeRef<'gc>>, bool) {
    match hit_at(cx, document, point, in_viewport_only) {
        Some(hit) => match hit.element {
            Some(element) => (cx.node_by_id(element), hit.text.is_some()),
            None => (document_element(cx, document), false),
        },
        None => (None, false),
    }
}

/// What the layout of `document`'s frame, laid out and pre-painted first,
/// hit at `point` (CSS px of the viewport); none without a frame, or
/// outside the viewport if `in_viewport_only`.
fn hit_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    point: PointF,
    in_viewport_only: bool,
) -> Option<Hit> {
    cx.with_layout(document, |layout| {
        let size = layout.viewport().size;
        if in_viewport_only
            && !((0.0..size.width).contains(&point.x) && (0.0..size.height).contains(&point.y))
        {
            return None;
        }
        Some(layout.hit_test(point))
    })
    .flatten()
}

/// `innerWidth`: the viewport's width, CSS px (with no scrollbars, all of
/// it); 0 if no frame shows the document. Its own layout doesn't decide
/// it, so none runs once it was laid out (as in Blink): a `resize` handler
/// that reads it after changing the style lays nothing out.
pub(crate) fn inner_width<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<i32, ApiError> {
    let document = cx.window_document(this);
    Ok(cx
        .viewport_size(document)
        .map_or(0, |size| saturated_cast(size.width)))
}

pub(crate) fn inner_height<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<i32, ApiError> {
    let document = cx.window_document(this);
    Ok(cx
        .viewport_size(document)
        .map_or(0, |size| saturated_cast(size.height)))
}

/// `outerWidth`: the browser window's width, CSS px; 0 if no browser
/// window shows the document (CSSOM View's "no client window"). It needs
/// no layout.
pub(crate) fn outer_width<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<i32, ApiError> {
    Ok(window_screen_rect(cx, this).map_or(0, |rect| saturated_cast(rect.width)))
}

pub(crate) fn outer_height<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<i32, ApiError> {
    Ok(window_screen_rect(cx, this).map_or(0, |rect| saturated_cast(rect.height)))
}

/// `screenX` (and `screenLeft`): where the browser window is on the
/// screen, CSS px; 0 if no browser window shows the document, as for
/// `outerWidth`.
pub(crate) fn screen_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Window>) -> Result<i32, ApiError> {
    Ok(window_screen_rect(cx, this).map_or(0, |rect| saturated_cast(rect.x)))
}

pub(crate) fn screen_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Window>) -> Result<i32, ApiError> {
    Ok(window_screen_rect(cx, this).map_or(0, |rect| saturated_cast(rect.y)))
}

fn window_screen_rect<'gc>(cx: &ApiCx<'_, 'gc>, window: Gc<'gc, Window>) -> Option<RectF> {
    cx.peek_layout(cx.window_document(window), |layout| {
        layout.window_screen_rect()
    })
    .flatten()
}

/// `scrollX` (and `pageXOffset`): how far the document is scrolled, laid
/// out first.
pub(crate) fn scroll_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Window>) -> Result<f64, ApiError> {
    let document = cx.window_document(this);
    Ok(cx
        .with_layout(document, |layout| f64::from(layout.viewport().scroll.x))
        .unwrap_or(0.0))
}

pub(crate) fn scroll_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Window>) -> Result<f64, ApiError> {
    let document = cx.window_document(this);
    Ok(cx
        .with_layout(document, |layout| f64::from(layout.viewport().scroll.y))
        .unwrap_or(0.0))
}

/// `devicePixelRatio`: 1 if no frame shows the document (the spec's "no
/// output device"). It needs no layout, so none runs (as in Blink).
pub(crate) fn device_pixel_ratio<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<f64, ApiError> {
    let document = cx.window_document(this);
    Ok(cx
        .peek_layout(document, |layout| {
            f64::from(layout.viewport().device_scale_factor)
        })
        .unwrap_or(1.0))
}

/// `node` (an element `document`'s frame hit, or in a frame below) as an
/// element of `document`: itself, or the iframe here its frame is in.
fn in_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    node: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    let mut node = node;
    // A frame tree is fewer than u16::MAX deep.
    for _ in 0..u16::MAX {
        if Gc::ptr_eq(cx.document_of(node), document) {
            return Some(node);
        }
        node = cx.frame_owner(cx.document_of(node))?;
    }
    None
}

/// Every element at `point` (CSS px of the viewport) of `document`'s, the
/// topmost first (`elementsFromPoint`), each once: those in a child frame
/// as the iframe here; none outside the viewport.
fn elements_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    point: PointF,
) -> Vec<NodeRef<'gc>> {
    let Some(hits) = cx
        .with_layout(document, |layout| {
            let size = layout.viewport().size;
            ((0.0..size.width).contains(&point.x) && (0.0..size.height).contains(&point.y))
                .then(|| layout.elements_at(point))
        })
        .flatten()
    else {
        return Vec::new();
    };
    let mut found: Vec<NodeRef<'gc>> = Vec::new();
    for hit in hits {
        if let Some(element) = cx
            .node_by_id(hit)
            .and_then(|node| in_document(cx, document, node))
            .filter(|&node| cx.kind(node) == NodeKind::Element)
            && !found.iter().any(|&seen| Gc::ptr_eq(seen, element))
        {
            found.push(element);
        }
    }
    // In the viewport, the root element last, below the content too
    // (Blink's `TreeScope::ElementsFromHitTestResult`).
    if let Some(root) = document_element(cx, document)
        && !found.iter().any(|&seen| Gc::ptr_eq(seen, root))
    {
        found.push(root);
    }
    found
}

/// `elementsFromPoint(x, y)` of a document or (`this` a shadow root) a
/// shadow root: each retargeted against it (CSSOM View's), once, the
/// topmost first.
pub(crate) fn elements_from_point<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    x: f64,
    y: f64,
) -> Result<Vec<NodeRef<'gc>>, ApiError> {
    let document = cx.document_of(this);
    let document = if cx.kind(this) == NodeKind::Document {
        this
    } else {
        document
    };
    let point = PointF::new(f32_from_f64(x), f32_from_f64(y));
    let mut found: Vec<NodeRef<'gc>> = Vec::new();
    for element in elements_at(cx, document, point) {
        let element = cx.retarget(element, Some(this));
        if !found.iter().any(|&seen| Gc::ptr_eq(seen, element)) {
            found.push(element);
        }
    }
    Ok(found)
}

/// A shadow root's `elementFromPoint(x, y)`: the document's, retargeted
/// against it.
pub(crate) fn shadow_element_from_point<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    x: f64,
    y: f64,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let document = cx.document_of(this);
    Ok(
        element_at(cx, document, PointF::new(f32_from_f64(x), f32_from_f64(y)))
            .map(|element| cx.retarget(element, Some(this))),
    )
}
