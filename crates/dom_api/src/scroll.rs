//! CSSOM View's members that scroll: `Element`'s `scrollTop` and
//! `scrollLeft` setters and its and `Window`'s `scrollTo`, `scroll` and
//! `scrollBy` (Blink's `Element::ScrollLayoutBoxTo` and `ScrollFrameTo`,
//! `LocalDOMWindow::scrollTo`). Each lays the frame out
//! first (but `Window`'s to (0, 0), as Blink's), works out where to scroll
//! to (a missing axis keeps its offset, a non-finite number is 0), snaps
//! it to device pixels as Blink does, and hands it to the frame's layout,
//! which clamps it to what can be scrolled (`FrameLayout::scroll_to`). And
//! `Element.scrollIntoView`, which the layout works out
//! (`FrameLayout::scroll_into_view`). Every scroll is instant: `behavior`
//! is checked, not obeyed, and the promise the methods return is resolved
//! once they have scrolled.

use dom::{NodeRef, Window};
use elise::Gc;
use gfx::{PointF, ScrollAlignment, ScrollIntoViewParams};

use crate::interfaces::{ScrollIntoViewOptions, ScrollToOptions};
use crate::typed::DictOrBool;
use crate::{ApiCx, ApiError};

/// A scroll offset from script: a non-finite number is 0 (Blink's
/// `NormalizeNonFiniteScroll`).
fn finite(value: f64) -> f32 {
    if value.is_finite() {
        base::numerics::f32_from_f64(value)
    } else {
        0.0
    }
}

/// WebIDL's conversion of a dictionary member of an enumeration: one of
/// `values`, else a `TypeError`.
fn check_enum(value: &str, values: &[&str], name: &str) -> Result<(), ApiError> {
    if values.contains(&value) {
        return Ok(());
    }
    Err(enum_error(value, name))
}

fn enum_error(value: &str, name: &str) -> ApiError {
    ApiError::type_error(format!(
        "The provided value '{value}' is not a valid enum value of type {name}."
    ))
}

/// `ScrollBehavior`'s values (read, not obeyed: every scroll is instant).
fn check_behavior(behavior: &str) -> Result<(), ApiError> {
    check_enum(behavior, &["auto", "instant", "smooth"], "ScrollBehavior")
}

/// Where script scrolls something from `current`: to `options`'s axes,
/// or by them; a missing axis stays.
fn target(current: PointF, left: Option<f64>, top: Option<f64>, by: bool) -> PointF {
    let axis = |current: f32, value: Option<f64>| match value {
        Some(v) if by => current + finite(v),
        Some(v) => finite(v),
        None => current,
    };
    PointF::new(axis(current.x, left), axis(current.y, top))
}

/// Scrolls the viewport of `document`'s frame, or (`scroller`) one of its
/// scroll containers, to `to`, snapped to device pixels (Blink's
/// `SnapScrollOffsetToPhysicalPixels`); the layout clamps it.
fn scroll<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, scroller: NodeRef<'gc>, to: PointF) {
    let (document_id, scroller_id) = (cx.node_id(document), cx.node_id(scroller));
    cx.lend_layout(document, |layout| {
        let scale = layout.viewport().device_scale_factor;
        let snap = |v: f32| (v * scale).round() / scale;
        layout.scroll_to(
            document_id,
            scroller_id,
            PointF::new(snap(to.x), snap(to.y)),
        );
    });
}

/// Scrolls `document`'s viewport to `left` and `top`, or by them (`by`),
/// laid out first.
fn scroll_viewport<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    left: Option<f64>,
    top: Option<f64>,
    by: bool,
) {
    let Some(current) = cx.with_layout(document, |layout| layout.viewport().scroll) else {
        return;
    };
    scroll(cx, document, document, target(current, left, top, by));
}

/// Scrolls `element` to `left` and `top`, or by them: the viewport if it
/// is the scrolling element; nothing if it isn't a scroll container. The
/// frame laid out once.
fn scroll_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    left: Option<f64>,
    top: Option<f64>,
    by: bool,
) {
    let document = cx.document_of(element);
    let id = cx.node_id(element);
    let found = cx.with_layout(element, |layout| {
        if crate::geometry::is_scrolling_element(cx, layout, element) {
            return Some((document, layout.viewport().scroll));
        }
        Some((element, layout.element_box(id)?.block?.scroll?))
    });
    if let Some((scroller, current)) = found.flatten() {
        scroll(cx, document, scroller, target(current, left, top, by));
    }
}

pub(crate) fn set_scroll_top<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: f64,
) -> Result<(), ApiError> {
    scroll_element(cx, this, None, Some(value), false);
    Ok(())
}

pub(crate) fn set_scroll_left<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: f64,
) -> Result<(), ApiError> {
    scroll_element(cx, this, Some(value), None, false);
    Ok(())
}

/// `Element.scrollTo(options)` and `scroll(options)`.
pub(crate) fn element_scroll_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &ScrollToOptions,
) -> Result<(), ApiError> {
    check_behavior(&options.behavior)?;
    scroll_element(cx, this, options.left, options.top, false);
    Ok(())
}

/// `Element.scrollTo(x, y)` and `scroll(x, y)`: both axes (Blink's
/// bindings make the same `ScrollToOptions`).
pub(crate) fn element_scroll_to_xy<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    scroll_element(cx, this, Some(x), Some(y), false);
    Ok(())
}

pub(crate) fn element_scroll_by<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &ScrollToOptions,
) -> Result<(), ApiError> {
    check_behavior(&options.behavior)?;
    scroll_element(cx, this, options.left, options.top, true);
    Ok(())
}

pub(crate) fn element_scroll_by_xy<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    scroll_element(cx, this, Some(x), Some(y), true);
    Ok(())
}

/// `Window.scrollTo(options)` and `scroll(options)`: its document's
/// viewport. Only a position that may be clamped needs the frame laid out
/// first, and (0, 0) never is, so scrolling there doesn't lay it out
/// (Blink's `LocalDOMWindow::scrollTo`): a page going back to its top
/// after changing its content leaves the layout to the next frame.
pub(crate) fn window_scroll_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    options: &ScrollToOptions,
) -> Result<(), ApiError> {
    check_behavior(&options.behavior)?;
    window_scroll_to_axes(cx, this, options.left, options.top);
    Ok(())
}

fn window_scroll_to_axes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    left: Option<f64>,
    top: Option<f64>,
) {
    let document = cx.window_document(this);
    if left == Some(0.0) && top == Some(0.0) {
        scroll(cx, document, document, PointF::new(0.0, 0.0));
    } else {
        scroll_viewport(cx, document, left, top, false);
    }
}

pub(crate) fn window_scroll_to_xy<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    window_scroll_to_axes(cx, this, Some(x), Some(y));
    Ok(())
}

pub(crate) fn window_scroll_by<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    options: &ScrollToOptions,
) -> Result<(), ApiError> {
    check_behavior(&options.behavior)?;
    scroll_viewport(
        cx,
        cx.window_document(this),
        options.left,
        options.top,
        true,
    );
    Ok(())
}

pub(crate) fn window_scroll_by_xy<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    scroll_viewport(cx, cx.window_document(this), Some(x), Some(y), true);
    Ok(())
}

/// `ScrollLogicalPosition`'s value `value`.
fn alignment(value: &str) -> Result<ScrollAlignment, ApiError> {
    Ok(match value {
        "start" => ScrollAlignment::Start,
        "center" => ScrollAlignment::Center,
        "end" => ScrollAlignment::End,
        "nearest" => ScrollAlignment::Nearest,
        other => return Err(enum_error(other, "ScrollLogicalPosition")),
    })
}

/// `Element.scrollIntoView(arg)`: every scrolling box around the element,
/// through the frames around its own, or only the nearest (Blink's
/// `Element::scrollIntoViewWithOptions`); `true` aligns the block axis's
/// start, `false` its end, the inline axis the nearest edge (Blink's
/// `ScrollIntoViewOptionsOrBoolean`). The frames are laid out first; an
/// element with no box doesn't scroll anything.
pub(crate) fn scroll_into_view<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    arg: &DictOrBool<ScrollIntoViewOptions>,
) -> Result<(), ApiError> {
    let params = match arg {
        DictOrBool::Bool(top) => ScrollIntoViewParams {
            block: if *top {
                ScrollAlignment::Start
            } else {
                ScrollAlignment::End
            },
            inline: ScrollAlignment::Nearest,
            nearest_only: false,
        },
        DictOrBool::Dict(options) => {
            check_behavior(&options.behavior)?;
            check_enum(
                &options.container,
                &["all", "nearest"],
                "ScrollIntoViewContainer",
            )?;
            ScrollIntoViewParams {
                block: alignment(&options.block)?,
                inline: alignment(&options.inline)?,
                nearest_only: options.container == "nearest",
            }
        }
    };
    cx.with_layout(this, |layout| layout.scroll_into_view(cx, this, params));
    Ok(())
}
