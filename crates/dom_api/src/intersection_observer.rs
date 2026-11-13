//! `IntersectionObserver` and `IntersectionObserverEntry` (Intersection
//! Observer, v2 included), over `dom`'s observers and observations;
//! what the rendering update does with the intersections the host computed
//! (`update_intersections`), and the task that delivers the entries
//! (`notify_intersection_observers`).

use dom::{
    IntersectionEntryValues, IntersectionObserver, IntersectionObserverEntry,
    IntersectionObserverOptions, IntersectionObserverRoot, NodeKind, NodeRef, Window,
};
use elise::{Gc, Persistent};
use gfx::RectF;

use crate::exception::DomException;
use crate::typed::{Callback, IntersectionEntryInit, IntersectionOptions, OpFront, ScriptHost};
use crate::{ApiCx, ApiError, DomStr, ExceptionName};

/// The least `delay` of an observer that tracks visibility.
const MIN_VISIBILITY_DELAY_MICROS: i64 = 100_000;

/// Blink's message for a `delay` too short for `trackVisibility`.
const TRACK_VISIBILITY: &str = "Failed to construct 'IntersectionObserver': To enable the \
    'trackVisibility' option, you must also use a 'delay' option with a value of at least 100. \
    Visibility is more expensive to compute than the basic intersection; enabling this option \
    may negatively affect your page's performance. Please make sure you *really* need \
    visibility tracking before enabling the 'trackVisibility' option.";

/// `new IntersectionObserver(callback, options)` (the spec's "initialize a
/// new IntersectionObserver"): one of the current realm's window's
/// observers. As Blink checks them: a `root` that isn't an element or a
/// document is a `TypeError`; `trackVisibility` with a `delay` under 100 ms
/// a `NotSupportedError`; then a margin that doesn't parse a `SyntaxError`,
/// a threshold outside 0 to 1 a `RangeError`. The thresholds are sorted,
/// `[0]` if none; the delay is whole microseconds, saturating (Blink's
/// `base::Milliseconds`).
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    callback: Callback<O::Callback>,
    options: IntersectionOptions<'gc>,
) -> Result<Gc<'gc, IntersectionObserver>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("Illegal constructor"))?;
    let IntersectionOptions { root, init } = options;
    if root.is_some_and(|root| !matches!(cx.kind(root), NodeKind::Element | NodeKind::Document)) {
        return Err(ApiError::type_error(
            "Failed to read the 'root' property from 'IntersectionObserverInit': The provided value is not of type '(Element or Document)'.",
        ));
    }
    let delay_micros: i64 = base::numerics::saturated_cast(init.delay * 1000.0);
    if init.track_visibility && delay_micros < MIN_VISIBILITY_DELAY_MICROS {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::NotSupported,
            message: TRACK_VISIBILITY.to_owned(),
        }));
    }
    let margin = |text: &str, name: &str| {
        style_resolver::parse_intersection_margin(text).ok_or_else(|| {
            ApiError::Dom(DomException {
                name: ExceptionName::Syntax,
                message: format!("{name} must be specified in pixels or percent."),
            })
        })
    };
    let root_margin = margin(&init.root_margin, "rootMargin")?;
    let scroll_margin = margin(&init.scroll_margin, "scrollMargin")?;
    let mut thresholds = init.thresholds;
    if thresholds.iter().any(|t| !(0.0..=1.0).contains(t)) {
        return Err(ApiError::Range(
            "Threshold values must be numbers between 0 and 1".into(),
        ));
    }
    thresholds.sort_by(f64::total_cmp);
    if thresholds.is_empty() {
        thresholds.push(0.0);
    }
    let observer = cx.create_intersection_observer(
        window,
        root,
        IntersectionObserverOptions {
            root_margin,
            scroll_margin,
            thresholds,
            delay_micros,
            track_visibility: init.track_visibility,
        },
    );
    op.store_callback(cx, cx.intersection_callback_edge(observer), callback);
    Ok(observer)
}

/// `new IntersectionObserverEntry(init)`: an entry of `init`'s values, its
/// `target` an element (a `TypeError` otherwise: WebIDL's `Element`).
pub(crate) fn construct_entry<'gc>(
    cx: &ApiCx<'_, 'gc>,
    init: IntersectionEntryInit<'gc>,
) -> Result<Gc<'gc, IntersectionObserverEntry>, ApiError> {
    let target = init
        .target
        .filter(|&target| cx.kind(target) == NodeKind::Element)
        .ok_or_else(|| {
            ApiError::type_error(
                "Failed to read the 'target' property from 'IntersectionObserverEntryInit': Failed to convert value to 'Element'.",
            )
        })?;
    let values = init.values;
    let entry = cx.create_intersection_entry(
        target,
        IntersectionEntryValues {
            time: values.time,
            root_bounds: values.root_bounds.map(|_| RectF::default()),
            bounding_client_rect: RectF::default(),
            intersection_rect: RectF::default(),
            is_intersecting: values.is_intersecting,
            is_visible: values.is_visible,
            intersection_ratio: values.intersection_ratio,
        },
    );
    // The rects' doubles as given (an entry's from the rendering update
    // are CSS px floats).
    let set = |rect, values: [f64; 4]| {
        for (index, value) in values.into_iter().enumerate() {
            cx.set_rect_value(rect, index, value);
        }
    };
    if let (Some(rect), Some(values)) =
        (cx.intersection_entry_root_bounds(entry), values.root_bounds)
    {
        set(rect, values);
    }
    set(
        cx.intersection_entry_bounding_client_rect(entry),
        values.bounding_client_rect,
    );
    set(
        cx.intersection_entry_intersection_rect(entry),
        values.intersection_rect,
    );
    Ok(entry)
}

/// `root`: the element or document it was made with; `null` for the
/// implicit root, and once that root is gone (Blink holds it weakly too).
pub(crate) fn root<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(match cx.intersection_observer_root(this) {
        IntersectionObserverRoot::Node(node) => Some(node),
        IntersectionObserverRoot::Implicit | IntersectionObserverRoot::Gone => None,
    })
}

/// `rootMargin`: its four sides, as whole px or percents.
pub(crate) fn root_margin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<DomStr<'gc>, ApiError> {
    let margin = style_resolver::serialize_intersection_margin(
        &cx.intersection_observer_options(this).root_margin,
    );
    Ok(DomStr::fresh(margin))
}

/// `scrollMargin`, as `rootMargin`.
pub(crate) fn scroll_margin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<DomStr<'gc>, ApiError> {
    let margin = style_resolver::serialize_intersection_margin(
        &cx.intersection_observer_options(this).scroll_margin,
    );
    Ok(DomStr::fresh(margin))
}

/// `thresholds`: sorted.
pub(crate) fn thresholds<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<Vec<f64>, ApiError> {
    Ok(cx.intersection_observer_options(this).thresholds.clone())
}

/// `delay`: in whole milliseconds, toward zero; one that saturated is the
/// end of `i64` (Blink's `TimeDelta::InMilliseconds`).
pub(crate) fn delay<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<f64, ApiError> {
    let micros = cx.intersection_observer_options(this).delay_micros;
    let millis = match micros {
        i64::MAX | i64::MIN => micros,
        _ => micros / 1000,
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a double is what script reads, as Blink's is"
    )]
    Ok(millis as f64)
}

/// `trackVisibility`.
pub(crate) fn track_visibility<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<bool, ApiError> {
    Ok(cx.intersection_observer_options(this).track_visibility)
}

/// `observe(target)`.
pub(crate) fn observe<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
    target: NodeRef<'gc>,
) -> Result<(), ApiError> {
    cx.observe_intersection(this, target);
    Ok(())
}

/// `unobserve(target)`.
pub(crate) fn unobserve<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
    target: NodeRef<'gc>,
) -> Result<(), ApiError> {
    cx.unobserve_intersection(this, target);
    Ok(())
}

/// `disconnect()`.
pub(crate) fn disconnect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<(), ApiError> {
    cx.disconnect_intersection_observer(this);
    Ok(())
}

/// `takeRecords()`: its queue, emptied.
pub(crate) fn take_records<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserver>,
) -> Result<Vec<Gc<'gc, IntersectionObserverEntry>>, ApiError> {
    Ok(cx.take_intersection_entries(this))
}

// --- `IntersectionObserverEntry` -------------------------------------------

pub(crate) fn time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<f64, ApiError> {
    Ok(cx.intersection_entry_values(this).0)
}

pub(crate) fn root_bounds<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<Option<Gc<'gc, dom::DomRect>>, ApiError> {
    Ok(cx.intersection_entry_root_bounds(this))
}

pub(crate) fn bounding_client_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<Gc<'gc, dom::DomRect>, ApiError> {
    Ok(cx.intersection_entry_bounding_client_rect(this))
}

pub(crate) fn intersection_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<Gc<'gc, dom::DomRect>, ApiError> {
    Ok(cx.intersection_entry_intersection_rect(this))
}

pub(crate) fn is_intersecting<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<bool, ApiError> {
    Ok(cx.intersection_entry_values(this).1)
}

pub(crate) fn intersection_ratio<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<f64, ApiError> {
    Ok(cx.intersection_entry_values(this).2)
}

pub(crate) fn is_visible<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<bool, ApiError> {
    Ok(cx.intersection_entry_values(this).3)
}

pub(crate) fn target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IntersectionObserverEntry>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.intersection_entry_target(this))
}

// --- The rendering update ---------------------------------------------------

/// What the rendering update computed for one observation, from the
/// geometry the host worked out (Blink's `IntersectionGeometry`): rects in
/// their documents' viewports, CSS px.
#[derive(Debug, Clone, Copy)]
pub struct ComputedIntersection<'gc> {
    pub observer: Gc<'gc, IntersectionObserver>,
    pub target: NodeRef<'gc>,
    /// The target's bounding box; `None` (zeros) if it has none, or isn't
    /// under an explicit root.
    pub target_rect: Option<RectF>,
    /// Its part in the root; `None` if it doesn't intersect it.
    pub intersection_rect: Option<RectF>,
    /// The root's rect, its margin applied; `None` for `null`.
    pub root_bounds: Option<RectF>,
    /// Whether the target is visible, for an observer that tracks it
    /// (Blink's `IntersectionGeometry::IsVisible`); `false` otherwise, and
    /// taken as `false` where it doesn't intersect (meets the root below
    /// the first threshold).
    pub is_visible: bool,
}

/// The spec's "run the update intersection observations steps", for
/// `window`'s observers, with what the host computed for each observation
/// at `time` (milliseconds on the window's timeline, `Cx::now_since`):
/// one whose threshold index (or whether it intersects, or is visible) is
/// not what it was last time gets an entry, queued on its observer. As
/// Chrome: a target intersects if its intersection meets a threshold (its
/// index isn't 0), so one that meets the root below the first threshold
/// doesn't (WPT's `isIntersecting-threshold.html`); the ratio of a target
/// with no area that meets the root is 1. Whether the task that delivers
/// them is to be queued (`notify_intersection_observers`: the first
/// observer waits for it).
pub fn update_intersections<'gc>(
    cx: &ApiCx<'_, 'gc>,
    window: Gc<'gc, Window>,
    computed: &[ComputedIntersection<'gc>],
    time: f64,
) -> bool {
    let mut queue_task = false;
    for c in computed {
        let previous = cx
            .intersection_observations(c.observer)
            .into_iter()
            .find(|o| elise::Gc::ptr_eq(o.target, c.target));
        // Unobserved since it was computed.
        let Some(previous) = previous else {
            continue;
        };
        let thresholds = &cx.intersection_observer_options(c.observer).thresholds;
        let area = |r: &RectF| f64::from(r.width) * f64::from(r.height);
        let target_area = c.target_rect.as_ref().map_or(0.0, area);
        let (ratio, index) = match &c.intersection_rect {
            None => (0.0, 0),
            Some(intersection) => {
                let ratio = if target_area > 0.0 {
                    (area(intersection) / target_area).min(1.0)
                } else {
                    1.0
                };
                let index = thresholds
                    .iter()
                    .position(|&t| t > ratio)
                    .unwrap_or(thresholds.len());
                (ratio, index)
            }
        };
        let is_intersecting = index > 0;
        // Blink computes it only for a target that intersects.
        let is_visible = is_intersecting && c.is_visible;
        if previous.previous_threshold_index == Some(index)
            && previous.previous_is_intersecting == is_intersecting
            && previous.previous_is_visible == is_visible
        {
            continue;
        }
        let entry = cx.create_intersection_entry(
            c.target,
            IntersectionEntryValues {
                // The computation's time, coarsened as `performance.now()`.
                time: crate::event::coarsen(time),
                root_bounds: c.root_bounds,
                bounding_client_rect: c.target_rect.unwrap_or_default(),
                intersection_rect: c.intersection_rect.unwrap_or_default(),
                is_intersecting,
                is_visible,
                intersection_ratio: ratio,
            },
        );
        cx.set_intersection_observation_state(
            c.observer,
            c.target,
            index,
            is_intersecting,
            is_visible,
        );
        queue_task |= cx.queue_intersection_entry(window, c.observer, entry);
    }
    queue_task
}

/// The spec's "notify intersection observers", `window`'s task: each
/// observer waiting for it, in the order it got its first entry, is called
/// back with its queue (emptied; one emptied by `takeRecords()` since is
/// skipped), `this` and the second argument the observer. An exception is
/// reported, and the next observer called.
pub fn notify_intersection_observers<H: ScriptHost>(host: &mut H, window: &Persistent<Window>) {
    let observers: Vec<Persistent<IntersectionObserver>> = host.mutate(|cx| {
        cx.take_pending_intersection_observers(window.get(cx.gc))
            .into_iter()
            .map(|observer| Persistent::new(cx.gc, observer))
            .collect()
    });
    for observer in observers {
        // A document's lazy load observer: its targets that intersect
        // resume their loads (Blink's `LazyLoadImageObserver::
        // LoadIfNearViewport`), no script called.
        let lazy_load = host.mutate(|cx| {
            let observer = observer.get(cx.gc);
            if !cx.is_lazy_load_observer(observer) {
                return false;
            }
            for entry in cx.take_intersection_entries(observer) {
                if cx.intersection_entry_values(entry).1 {
                    cx.resume_lazy_image(cx.intersection_entry_target(entry));
                }
            }
            true
        });
        if lazy_load {
            continue;
        }
        let entries: Vec<Persistent<IntersectionObserverEntry>> = host.mutate(|cx| {
            cx.take_intersection_entries(observer.get(cx.gc))
                .into_iter()
                .map(|entry| Persistent::new(cx.gc, entry))
                .collect()
        });
        if entries.is_empty() {
            continue;
        }
        host.call_intersection_callback(&observer, &entries);
    }
}
