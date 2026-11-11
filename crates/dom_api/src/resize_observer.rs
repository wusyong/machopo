//! `ResizeObserver`, `ResizeObserverEntry` and `ResizeObserverSize`
//! (Resize Observer), over `dom`'s observers and observations; and
//! what the rendering update does with them in a window's document: gather
//! the observations whose size changed at a depth and broadcast them
//! (`gather_and_broadcast`), and the loop error (`report_loop_error`). The
//! host runs the loop, laying the frame out before each round (Blink's
//! `LocalFrameView::NotifyResizeObservers` in its lifecycle's loop).

use dom::{
    BoxSize, ErrorInfo, NodeKind, NodeRef, ResizeObserver, ResizeObserverBox, ResizeObserverEntry,
    ResizeObserverEntryValues, ResizeObserverSize, Window,
};
use elise::{Gc, Persistent};

use crate::typed::{Callback, OpFront, ScriptHost};
use crate::{ApiCx, ApiError};

/// `new ResizeObserver(callback)`: one of the current realm's window's
/// observers (the spec adds it to the document's `[[resizeObservers]]`).
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    callback: Callback<O::Callback>,
) -> Result<Gc<'gc, ResizeObserver>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("Illegal constructor"))?;
    let observer = cx.create_resize_observer(window);
    op.store_callback(cx, cx.resize_callback_edge(observer), callback);
    Ok(observer)
}

/// `observe(target, options)`: `box` one of `ResizeObserverBoxOptions`'
/// values (WebIDL's enumeration: another is a `TypeError`).
pub(crate) fn observe<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserver>,
    target: NodeRef<'gc>,
    options: &crate::interfaces::ResizeObserverOptions,
) -> Result<(), ApiError> {
    let observed_box = match options.r#box.as_str() {
        "content-box" => ResizeObserverBox::ContentBox,
        "border-box" => ResizeObserverBox::BorderBox,
        "device-pixel-content-box" => ResizeObserverBox::DevicePixelContentBox,
        other => {
            return Err(ApiError::type_error(format!(
                "Failed to execute 'observe' on 'ResizeObserver': Failed to read the 'box' property from 'ResizeObserverOptions': The provided value '{other}' is not a valid enum value of type ResizeObserverBoxOptions."
            )));
        }
    };
    cx.observe_resize(this, target, observed_box);
    Ok(())
}

pub(crate) fn unobserve<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserver>,
    target: NodeRef<'gc>,
) -> Result<(), ApiError> {
    cx.unobserve_resize(this, target);
    Ok(())
}

pub(crate) fn disconnect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserver>,
) -> Result<(), ApiError> {
    cx.disconnect_resize_observer(this);
    Ok(())
}

// --- `ResizeObserverEntry` and `ResizeObserverSize` -------------------------

pub(crate) fn target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverEntry>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.resize_entry_target(this))
}

pub(crate) fn content_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverEntry>,
) -> Result<Gc<'gc, dom::DomRect>, ApiError> {
    Ok(cx.resize_entry_content_rect(this))
}

fn sizes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverEntry>,
    observed_box: ResizeObserverBox,
) -> Result<Vec<Gc<'gc, ResizeObserverSize>>, ApiError> {
    Ok(vec![cx.resize_entry_size(this, observed_box)])
}

pub(crate) fn border_box_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverEntry>,
) -> Result<Vec<Gc<'gc, ResizeObserverSize>>, ApiError> {
    sizes(cx, this, ResizeObserverBox::BorderBox)
}

pub(crate) fn content_box_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverEntry>,
) -> Result<Vec<Gc<'gc, ResizeObserverSize>>, ApiError> {
    sizes(cx, this, ResizeObserverBox::ContentBox)
}

pub(crate) fn device_pixel_content_box_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverEntry>,
) -> Result<Vec<Gc<'gc, ResizeObserverSize>>, ApiError> {
    sizes(cx, this, ResizeObserverBox::DevicePixelContentBox)
}

pub(crate) fn inline_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverSize>,
) -> Result<f64, ApiError> {
    Ok(cx.resize_size(this).inline)
}

pub(crate) fn block_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ResizeObserverSize>,
) -> Result<f64, ApiError> {
    Ok(cx.resize_size(this).block)
}

// --- The rendering update ---------------------------------------------------

/// What a round of the rendering update's loop did in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResizeObserverRound {
    /// The shallowest depth broadcast (the spec's "broadcast active resize
    /// observations"), the document's depth for the next round; `None` if
    /// no observation was active.
    pub broadcast: Option<usize>,
    /// Whether an observation's size changed but it wasn't deeper than
    /// the round's depth, so it waits (the spec's "has skipped
    /// observations").
    pub skipped: bool,
}

/// The spec's "gather active resize observations at depth" and "broadcast
/// active resize observations" for `window`'s observers, its layout up to
/// date: each observer with an observation whose observed box's size
/// isn't what it last reported, and deeper than `depth`, is called back
/// with an entry for each, in its observations' order, the observers in
/// the order they were made. As Blink's `ResizeObserver::
/// DeliverObservations`: an observer's entries, and the sizes they
/// report, are read from the layout as it is called (a callback before it
/// that laid the document out changes them; none lays it out again).
pub fn gather_and_broadcast<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    depth: usize,
) -> ResizeObserverRound {
    let mut round = ResizeObserverRound::default();
    let gathered: Vec<(Persistent<ResizeObserver>, Vec<dom::NodeRoot>)> = host.mutate(|cx| {
        let mut gathered = Vec::new();
        for observer in cx.window_resize_observers(window.get(cx.gc)) {
            let mut active = Vec::new();
            for observation in cx.resize_observations(observer) {
                let size = observed_size(
                    &entry_values(cx, observation.target),
                    observation.observed_box,
                );
                if observation.last_reported == Some(size) {
                    continue;
                }
                let target_depth = depth_of(cx, observation.target);
                if target_depth > depth {
                    active.push(cx.root(observation.target));
                    round.broadcast = Some(
                        round
                            .broadcast
                            .map_or(target_depth, |d| d.min(target_depth)),
                    );
                } else {
                    round.skipped = true;
                }
            }
            if !active.is_empty() {
                gathered.push((Persistent::new(cx.gc, observer), active));
            }
        }
        gathered
    });
    for (observer, targets) in gathered {
        // A document's observer of its lazy `<img>`s with `sizes="auto"`:
        // each still so, and rendered, selects its source again (Blink's
        // `HTMLImageElement::OnResize`; one that stopped being rendered
        // isn't a relevant mutation), no script called.
        let auto_sizes = host.mutate(|cx| {
            let observer = observer.get(cx.gc);
            if !cx.is_auto_sizes_observer(observer) {
                return false;
            }
            for target in &targets {
                let target = target.get(cx.gc);
                let values = entry_values(cx, target);
                cx.set_resize_observation_reported(observer, target, values.content_box);
                let rendered = cx
                    .peek_layout(target, |layout| {
                        layout.element_box(cx.node_id(target)).is_some()
                    })
                    .unwrap_or(false);
                if rendered
                    && cx.allows_auto_sizes(target)
                    && cx.image_uses_srcset_or_picture(target)
                {
                    crate::image::select_again(cx, target);
                }
            }
            true
        });
        if auto_sizes {
            continue;
        }
        let entries: Vec<Persistent<ResizeObserverEntry>> = host.mutate(|cx| {
            let observer = observer.get(cx.gc);
            let observations = cx.resize_observations(observer);
            targets
                .iter()
                .filter_map(|target| {
                    let target = target.get(cx.gc);
                    // Unobserved by a callback before this one.
                    let observation = observations
                        .iter()
                        .find(|observation| Gc::ptr_eq(observation.target, target))?;
                    let values = entry_values(cx, target);
                    cx.set_resize_observation_reported(
                        observer,
                        target,
                        observed_size(&values, observation.observed_box),
                    );
                    Some(Persistent::new(
                        cx.gc,
                        cx.create_resize_entry(target, values),
                    ))
                })
                .collect()
        });
        if !entries.is_empty() {
            host.call_resize_callback(&observer, &entries);
            host.perform_microtask_checkpoint();
        }
    }
    round
}

/// The spec's "deliver resize loop error notification": an `ErrorEvent`
/// reported at `window` as an exception would be (Blink's message), with
/// no error.
pub fn report_loop_error<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
) -> crate::Reported {
    let info = ErrorInfo {
        message: dom::DomString::new(
            "ResizeObserver loop completed with undelivered notifications.",
        ),
        filename: dom::DomString::new(""),
        lineno: 0,
        colno: 0,
    };
    crate::report_exception(host, window, &info, None)
}

/// Whether `window`'s document has a resize observer observing anything:
/// the host's loop runs only then.
pub fn has_resize_observations<'gc>(cx: &ApiCx<'_, 'gc>, window: Gc<'gc, Window>) -> bool {
    cx.window_resize_observers(window)
        .into_iter()
        .any(|observer| !cx.resize_observations(observer).is_empty())
}

fn observed_size(values: &ResizeObserverEntryValues, observed_box: ResizeObserverBox) -> BoxSize {
    match observed_box {
        ResizeObserverBox::ContentBox => values.content_box,
        ResizeObserverBox::BorderBox => values.border_box,
        ResizeObserverBox::DevicePixelContentBox => values.device_pixel_content_box,
    }
}

/// `target`'s sizes, as last laid out (the spec's "calculate box size",
/// and "create and populate a `ResizeObserverEntry`"'s rect): a block box's
/// content and border boxes, the content rect at the padding's offset;
/// 0×0 for an inline element that isn't replaced, one with no box, and one
/// in a document no frame shows. machopo has no zoom or writing modes. The
/// device-pixel content box is the content box in device pixels, snapped
/// as Chrome's `ResizeObserverUtilities::ComputeSnappedDevicePixelContentBox`
/// does: each side rounded from where the border box's paint offset puts
/// it (Blink snaps the content size with the border box's offset).
fn entry_values<'gc>(cx: &ApiCx<'_, 'gc>, target: NodeRef<'gc>) -> ResizeObserverEntryValues {
    let id = cx.node_id(target);
    let (element_box, scale, offset) = cx
        .peek_layout(target, |layout| {
            (
                layout.element_box(id),
                layout.viewport().device_scale_factor,
                layout.paint_offset(id),
            )
        })
        .unwrap_or((None, 1.0, None));
    let Some((border_box, block)) =
        element_box.and_then(|b| b.block.map(|block| (b.border_box, block)))
    else {
        return svg_entry_values(cx, target, scale);
    };
    let [top, right, bottom, left] = block.border;
    let [padding_top, padding_right, padding_bottom, padding_left] = block.padding;
    let width = (border_box.width - left - right - padding_left - padding_right).max(0.0);
    let height = (border_box.height - top - bottom - padding_top - padding_bottom).max(0.0);
    let content = BoxSize {
        inline: f64::from(width),
        block: f64::from(height),
    };
    let scale = f64::from(scale);
    // Blink's `SnapSizeToPixel`: the far edge rounded, less the near one.
    let snap = |length: f64, at: f32| {
        let at = f64::from(at) * scale;
        (at + length * scale).round() - at.round()
    };
    let at = offset.unwrap_or(gfx::PointF::new(0.0, 0.0));
    ResizeObserverEntryValues {
        content_rect: [
            f64::from(padding_left),
            f64::from(padding_top),
            content.inline,
            content.block,
        ],
        border_box: BoxSize {
            inline: f64::from(border_box.width),
            block: f64::from(border_box.height),
        },
        content_box: content,
        device_pixel_content_box: BoxSize {
            inline: snap(content.inline, at.x),
            block: snap(content.block, at.y),
        },
    }
}

/// An SVG element's, which has no CSS box: its bounding box, if it is
/// rendered, as each box, at the origin (Blink's
/// `ResizeObserverUtilities::ComputeZoomAdjustedBox` for an
/// `SVGGraphicsElement`); the device-pixel one its size rounded, as from
/// an offset of 0. Nothing for another element.
fn svg_entry_values<'gc>(
    cx: &ApiCx<'_, 'gc>,
    target: NodeRef<'gc>,
    scale: f32,
) -> ResizeObserverEntryValues {
    let is_svg = cx
        .qualified_name(target)
        .is_some_and(|name| name.namespace() == dom::names::SVG_NAMESPACE);
    let bbox = is_svg
        .then(|| cx.peek_layout(target, |layout| layout.svg_geometry(cx, target)))
        .flatten()
        .filter(|geometry| geometry.client_rect.is_some())
        .map(|geometry| geometry.bbox);
    let Some(bbox) = bbox else {
        return ResizeObserverEntryValues::default();
    };
    let size = BoxSize {
        inline: f64::from(bbox.width),
        block: f64::from(bbox.height),
    };
    let scale = f64::from(scale);
    ResizeObserverEntryValues {
        content_rect: [0.0, 0.0, size.inline, size.block],
        border_box: size,
        content_box: size,
        device_pixel_content_box: BoxSize {
            inline: (size.inline * scale).round(),
            block: (size.block * scale).round(),
        },
    }
}

/// The spec's "calculate depth for node": the elements from `node` up to
/// the root of its flat tree (Blink's `ResizeObservation::TargetDepth`,
/// `FlatTreeTraversal::ParentElement`): a node assigned to a slot is
/// deeper than its host.
fn depth_of<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> usize {
    std::iter::successors(Some(node), |&node| cx.flat_tree_parent(node))
        .filter(|&node| cx.kind(node) == NodeKind::Element)
        .count()
}
