//! `Performance` (https://w3c.github.io/hr-time/#sec-performance), its
//! timeline (Performance Timeline's entries and `PerformanceObserver`),
//! User Timing's marks and measures, and Navigation Timing's
//! `PerformanceNavigationTiming`, `performance.timing` and
//! `performance.navigation` (Blink's `core/timing`). A window's `performance` is on its timeline: times since its
//! time origin, coarsened (`coarsen`). Its document's navigation timing is
//! the window's (`dom`'s: its host's, and the document's own marked as
//! its events fire).

use dom::{
    DomString, EventTargetImpl, NavigationTiming, NavigationType, NodeRoot, PerformanceEntry,
    PerformanceEntryType, PerformanceObserver, PerformanceObserverEntryList,
    PerformanceObserverMode, PerformanceTiming, PerformanceTimingKind, Window,
};
use elise::{Gc, Persistent};

use crate::interfaces::{PerformanceMarkOptions, PerformanceObserverInit};
use crate::typed::{Callback, MeasureStartParam, OpFront, ScriptHost, StringOrNumber};
use crate::{ApiCx, ApiError, DomException, DomStr, ExceptionName, coarsen};

/// `performance`: the same object each time, kept by the window, with its
/// document's navigation entry in its buffer from the first time script
/// reaches it.
pub(crate) fn window_performance<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let performance = cx.window_performance(this);
    ensure_navigation_entry(cx, this, performance);
    Ok(performance)
}

/// HR-Time's "current high resolution time" of `window`: the time since
/// its time origin, coarsened.
fn window_now<'gc>(cx: &ApiCx<'_, 'gc>, window: Gc<'gc, Window>) -> f64 {
    coarsen(cx.now_since(Some(window)))
}

/// `now()`.
pub(crate) fn now<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<f64, ApiError> {
    Ok(window_now(cx, cx.performance_window(this)))
}

/// `timeOrigin`: the time origin's Unix time, in milliseconds, coarsened.
pub(crate) fn time_origin<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<f64, ApiError> {
    Ok(coarsen(
        cx.window_unix_time_origin(cx.performance_window(this)),
    ))
}

// --- Performance Timeline ----------------------------------------------------------

/// The buffer in `startTime` order (a stable sort: entries of the same
/// time in the order they came).
fn entries<'gc>(
    cx: &ApiCx<'_, 'gc>,
    performance: Gc<'gc, EventTargetImpl>,
) -> Vec<Gc<'gc, PerformanceEntry>> {
    let mut entries = cx.performance_entries(performance);
    entries.sort_by(|&a, &b| start_time_of(cx, a).total_cmp(&start_time_of(cx, b)));
    entries
}

fn start_time_of<'gc>(cx: &ApiCx<'_, 'gc>, entry: Gc<'gc, PerformanceEntry>) -> f64 {
    cx.performance_entry_times(entry).0
}

/// `entries` of `entry_type` (none for a type machopo has no entries of),
/// named `name` if given.
fn filter<'gc>(
    cx: &ApiCx<'_, 'gc>,
    mut entries: Vec<Gc<'gc, PerformanceEntry>>,
    name: Option<&str>,
    entry_type: Option<&str>,
) -> Vec<Gc<'gc, PerformanceEntry>> {
    entries.retain(|&entry| {
        name.is_none_or(|name| *cx.performance_entry_name(entry).to_str() == *name)
            && entry_type
                .is_none_or(|entry_type| cx.performance_entry_type(entry).name() == entry_type)
    });
    entries
}

pub(crate) fn get_entries<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(entries(cx, this))
}

pub(crate) fn get_entries_by_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    entry_type: &str,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(filter(cx, entries(cx, this), None, Some(entry_type)))
}

pub(crate) fn get_entries_by_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    name: &str,
    entry_type: Option<&str>,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(filter(cx, entries(cx, this), Some(name), entry_type))
}

pub(crate) fn name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.performance_entry_name(this).clone()))
}

pub(crate) fn entry_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh(cx.performance_entry_type(this).name()))
}

pub(crate) fn start_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<f64, ApiError> {
    Ok(start_time_of(cx, this))
}

/// `duration`: a navigation entry's grows to its `loadEventEnd`.
pub(crate) fn duration<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<f64, ApiError> {
    Ok(match cx.performance_entry_type(this) {
        PerformanceEntryType::Navigation => relative(entry_timing(cx, this).load_event_end),
        PerformanceEntryType::Mark | PerformanceEntryType::Measure => {
            cx.performance_entry_times(this).1
        }
    })
}

/// A mark's or a measure's `detail`: `null`, or its deserialization, the
/// same object each time (`EntryDetail`).
pub(crate) fn detail<'gc>(
    _: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<Gc<'gc, PerformanceEntry>, ApiError> {
    Ok(this)
}

// --- User Timing ----------------------------------------------------------------------

/// The `PerformanceTiming` attributes' names (Navigation Timing Level 1):
/// User Timing takes them as the times they name.
const TIMING_NAMES: &[&str] = &[
    "navigationStart",
    "unloadEventStart",
    "unloadEventEnd",
    "redirectStart",
    "redirectEnd",
    "fetchStart",
    "domainLookupStart",
    "domainLookupEnd",
    "connectStart",
    "connectEnd",
    "secureConnectionStart",
    "requestStart",
    "responseStart",
    "responseEnd",
    "domLoading",
    "domInteractive",
    "domContentLoadedEventStart",
    "domContentLoadedEventEnd",
    "domComplete",
    "loadEventStart",
    "loadEventEnd",
];

fn dom_error(name: ExceptionName, message: String) -> ApiError {
    ApiError::Dom(DomException { name, message })
}

/// The `PerformanceMark` constructor's steps, for a mark of `window`'s: a
/// timing name is a `SyntaxError`; a given `startTime` (not negative) or
/// now.
fn make_mark<'gc>(
    cx: &ApiCx<'_, 'gc>,
    window: Gc<'gc, Window>,
    name: &str,
    options: &PerformanceMarkOptions,
) -> Result<Gc<'gc, PerformanceEntry>, ApiError> {
    if TIMING_NAMES.contains(&name) {
        return Err(dom_error(
            ExceptionName::Syntax,
            format!(
                "'{name}' is part of the PerformanceTiming interface, and cannot be used as a mark name."
            ),
        ));
    }
    let start_time = match options.startTime {
        Some(time) if time < 0.0 => {
            return Err(ApiError::type_error("startTime can't be negative"));
        }
        Some(time) => time,
        None => window_now(cx, window),
    };
    let detail = options.detail.as_ref().map(|detail| detail.0.clone());
    Ok(cx.create_performance_entry(
        window,
        PerformanceEntryType::Mark,
        name,
        start_time,
        0.0,
        detail,
    ))
}

/// `new PerformanceMark(markName, markOptions)`: of the realm making it, in
/// no buffer.
pub(crate) fn construct_mark<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    name: &str,
    options: &PerformanceMarkOptions,
) -> Result<Gc<'gc, PerformanceEntry>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("PerformanceMark needs a window"))?;
    make_mark(cx, window, name, options)
}

/// `mark(markName, markOptions)`: a new mark, queued in the buffer.
pub(crate) fn mark<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    name: &str,
    options: &PerformanceMarkOptions,
) -> Result<Gc<'gc, PerformanceEntry>, ApiError> {
    let mark = make_mark(cx, cx.performance_window(this), name, options)?;
    queue_entry(cx, this, mark);
    Ok(mark)
}

pub(crate) fn clear_marks<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    name: Option<&str>,
) -> Result<(), ApiError> {
    cx.clear_performance_entries(this, PerformanceEntryType::Mark, name);
    Ok(())
}

pub(crate) fn clear_measures<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    name: Option<&str>,
) -> Result<(), ApiError> {
    cx.clear_performance_entries(this, PerformanceEntryType::Measure, name);
    Ok(())
}

/// User Timing's "convert a mark to a timestamp": a time as it is (not
/// negative); a timing name `performance.timing`'s time of it, less
/// `navigationStart` (0, not happened yet, an `InvalidAccessError`); else
/// the latest mark of that name, or a `SyntaxError`.
fn mark_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    performance: Gc<'gc, EventTargetImpl>,
    mark: &StringOrNumber,
) -> Result<f64, ApiError> {
    let name = match mark {
        StringOrNumber::Number(time) if *time < 0.0 => {
            return Err(ApiError::type_error("a time can't be negative"));
        }
        StringOrNumber::Number(time) => return Ok(*time),
        StringOrNumber::String(name) => name.as_str(),
    };
    if name == "navigationStart" {
        return Ok(0.0);
    }
    if TIMING_NAMES.contains(&name) {
        let window = cx.performance_window(performance);
        return match (
            legacy_time(cx, window, name),
            legacy_time(cx, window, "navigationStart"),
        ) {
            #[expect(
                clippy::cast_precision_loss,
                reason = "milliseconds since the epoch: far below 2^53"
            )]
            (time, start) if time > 0 => Ok(time.saturating_sub(start) as f64),
            _ => Err(dom_error(
                ExceptionName::InvalidAccess,
                format!(
                    "'{name}' is empty: either the event hasn't happened yet, or it would provide cross-origin timing information."
                ),
            )),
        };
    }
    cx.performance_entries(performance)
        .into_iter()
        .rev()
        .find(|&entry| {
            cx.performance_entry_type(entry) == PerformanceEntryType::Mark
                && *cx.performance_entry_name(entry).to_str() == *name
        })
        .map(|entry| start_time_of(cx, entry))
        .ok_or_else(|| {
            dom_error(
                ExceptionName::Syntax,
                format!("The mark '{name}' does not exist"),
            )
        })
}

/// `measure(measureName, startOrMeasureOptions, endMark)`: User Timing
/// Level 3's steps. Options with any member exclude `endMark`, need a
/// start or an end, and can't have all of start, duration and end; the end
/// is `endMark`, the options' end, their start plus duration, or now; the
/// start the options' start, their end less duration, the mark named, or 0.
pub(crate) fn measure<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    name: &str,
    start_or_options: MeasureStartParam<'_>,
    end_mark: Option<&str>,
) -> Result<Gc<'gc, PerformanceEntry>, ApiError> {
    let (start_mark, options) = match start_or_options {
        MeasureStartParam::Mark(start) => (Some(start), None),
        MeasureStartParam::Options(options) => (None, Some(options)),
        MeasureStartParam::None => (None, None),
    };
    let options = options.filter(|o| {
        o.start.is_some() || o.end.is_some() || o.duration.is_some() || o.detail.is_some()
    });
    if let Some(options) = options {
        if end_mark.is_some() {
            return Err(ApiError::type_error(
                "options and an end mark can't both be given",
            ));
        }
        if options.start.is_none() && options.end.is_none() {
            return Err(ApiError::type_error("the options need a start or an end"));
        }
        if options.start.is_some() && options.duration.is_some() && options.end.is_some() {
            return Err(ApiError::type_error(
                "the options can't have a start, a duration and an end",
            ));
        }
    }
    let duration = match options.and_then(|o| o.duration) {
        Some(duration) if duration < 0.0 => {
            return Err(ApiError::type_error("a duration can't be negative"));
        }
        other => other,
    };
    let window = cx.performance_window(this);
    let end = if let Some(end_mark) = end_mark {
        mark_time(cx, this, &StringOrNumber::String(end_mark.to_owned()))?
    } else if let Some(end) = options.and_then(|o| o.end.as_ref()) {
        mark_time(cx, this, end)?
    } else if let (Some(start), Some(duration)) = (options.and_then(|o| o.start.as_ref()), duration)
    {
        mark_time(cx, this, start)? + duration
    } else {
        window_now(cx, window)
    };
    let start = if let Some(start) = options.and_then(|o| o.start.as_ref()) {
        mark_time(cx, this, start)?
    } else if let (Some(duration), Some(end)) = (duration, options.and_then(|o| o.end.as_ref())) {
        mark_time(cx, this, end)? - duration
    } else if let Some(start) = start_mark {
        mark_time(cx, this, &StringOrNumber::String(start.to_owned()))?
    } else {
        0.0
    };
    let detail = options
        .and_then(|o| o.detail.as_ref())
        .map(|detail| detail.0.clone());
    let measure = cx.create_performance_entry(
        window,
        PerformanceEntryType::Measure,
        name,
        start,
        end - start,
        detail,
    );
    queue_entry(cx, this, measure);
    Ok(measure)
}

// --- PerformanceObserver --------------------------------------------------------------

/// The entry types machopo makes, in `supportedEntryTypes`' order (sorted).
const SUPPORTED: [(&str, PerformanceEntryType); 3] = [
    ("mark", PerformanceEntryType::Mark),
    ("measure", PerformanceEntryType::Measure),
    ("navigation", PerformanceEntryType::Navigation),
];

fn supported(name: &str) -> Option<PerformanceEntryType> {
    SUPPORTED
        .iter()
        .find(|(supported, _)| *supported == name)
        .map(|&(_, entry_type)| entry_type)
}

/// `name`, an entry type machopo doesn't make, in the page's console as
/// Chrome 152 warns of it (`supported`'s `None`).
fn warn_unsupported(cx: &ApiCx<'_, '_>, name: &str) {
    cx.warn_console(format!(
        "The entry type '{name}' does not exist or isn't supported."
    ));
}

/// `new PerformanceObserver(callback)`, in the realm making it.
pub(crate) fn construct_observer<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    callback: Callback<O::Callback>,
) -> Result<Gc<'gc, PerformanceObserver>, ApiError> {
    let window = op.current_window(cx).ok_or_else(|| {
        dom_error(
            ExceptionName::InvalidState,
            "no realm to make the PerformanceObserver in".to_owned(),
        )
    })?;
    let observer = cx.create_performance_observer(window);
    op.store_callback(
        cx,
        cx.performance_observer_callback_edge(observer),
        callback,
    );
    Ok(observer)
}

fn observer_performance<'gc>(
    cx: &ApiCx<'_, 'gc>,
    observer: Gc<'gc, PerformanceObserver>,
) -> Gc<'gc, EventTargetImpl> {
    let window = cx.performance_observer_window(observer);
    let performance = cx.window_performance(window);
    ensure_navigation_entry(cx, window, performance);
    performance
}

fn invalid_modification(message: &str) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::InvalidModification,
        message: message.to_owned(),
    })
}

/// `observe(options)`: Performance Timeline's steps, with Chrome 152's
/// messages. `entryTypes` replaces the types observed (a `buffered` with
/// it is warned of in the console, and ignored); `type` adds one, and with
/// `buffered` its entries already in the buffer, in `startTime` order. A
/// type machopo doesn't make is warned of and left out; nothing left, nothing
/// observed. An observer asks one way only: the other is an
/// `InvalidModificationError`.
pub(crate) fn observe<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceObserver>,
    options: &PerformanceObserverInit,
) -> Result<(), ApiError> {
    let mode = cx.performance_observer_mode(this);
    let (types, buffered) = match (&options.entryTypes, &options.r#type) {
        (None, None) => {
            return Err(ApiError::type_error(
                "An observe() call must include either entryTypes or type arguments.",
            ));
        }
        (Some(_), Some(_)) => {
            return Err(ApiError::type_error(
                "An observe() call must not include both entryTypes and type arguments.",
            ));
        }
        (Some(entry_types), None) => {
            if mode == PerformanceObserverMode::Single {
                return Err(invalid_modification(
                    "This PerformanceObserver has performed observe({type:...}, therefore it cannot perform observe({entryTypes:...})",
                ));
            }
            cx.set_performance_observer_mode(this, PerformanceObserverMode::Multiple);
            let mut types = Vec::new();
            for name in entry_types {
                match supported(name) {
                    Some(entry_type) => types.push(entry_type),
                    None => warn_unsupported(cx, name),
                }
            }
            if options.buffered == Some(true) {
                cx.warn_console(
                    "The PerformanceObserver does not support buffered flag with the entryTypes \
                     argument."
                        .to_owned(),
                );
            }
            (types, false)
        }
        (None, Some(entry_type)) => {
            if mode == PerformanceObserverMode::Multiple {
                return Err(invalid_modification(
                    "This observer has performed observe({entryTypes:...}, therefore it cannot perform observe({type:...})",
                ));
            }
            cx.set_performance_observer_mode(this, PerformanceObserverMode::Single);
            match supported(entry_type) {
                Some(entry_type) => (vec![entry_type], options.buffered == Some(true)),
                None => {
                    warn_unsupported(cx, entry_type);
                    (Vec::new(), false)
                }
            }
        }
    };
    if types.is_empty() {
        return Ok(());
    }
    let performance = observer_performance(cx, this);
    cx.observe_performance(performance, this, &types);
    if buffered {
        let mut buffered = entries(cx, performance);
        buffered.retain(|&entry| types.contains(&cx.performance_entry_type(entry)));
        if !buffered.is_empty() {
            for entry in buffered {
                cx.queue_performance_observer_entry(performance, this, entry);
            }
            cx.queue_performance_observer_task(performance);
        }
    }
    Ok(())
}

/// `disconnect()`.
pub(crate) fn disconnect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceObserver>,
) -> Result<(), ApiError> {
    let performance = observer_performance(cx, this);
    cx.disconnect_performance_observer(performance, this);
    Ok(())
}

/// `takeRecords()`: the buffer as it came, emptied.
pub(crate) fn take_records<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceObserver>,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(cx.take_performance_observer_entries(this))
}

/// `supportedEntryTypes`: the types machopo makes, sorted.
pub(crate) fn supported_entry_types<'gc>(_: &ApiCx<'_, 'gc>) -> Result<Vec<DomString>, ApiError> {
    Ok(SUPPORTED
        .iter()
        .map(|&(name, _)| DomString::new(name))
        .collect())
}

/// Performance Timeline's "queue a PerformanceEntry" (a mark's or a
/// measure's: one that goes in the buffer): the observers have it, then
/// the buffer.
fn queue_entry<'gc>(
    cx: &ApiCx<'_, 'gc>,
    performance: Gc<'gc, EventTargetImpl>,
    entry: Gc<'gc, PerformanceEntry>,
) {
    queue_to_observers(cx, performance, entry);
    cx.add_performance_entry(performance, entry);
    trace_entry(cx, performance, entry);
}

/// A mark or a measure queued, in machopo's trace (target `blink.user_timing`,
/// which `shell`'s trace writes as Chrome's has it): at the entry's times,
/// given as how long before now (ns; negative is after), so that whoever
/// writes it needn't share the window's clock. Blink's `UserTiming` writes
/// the trace event as it adds the entry. Nothing is computed unless the
/// trace takes the event.
fn trace_entry<'gc>(
    cx: &ApiCx<'_, 'gc>,
    performance: Gc<'gc, EventTargetImpl>,
    entry: Gc<'gc, PerformanceEntry>,
) {
    if !tracing::enabled!(target: "blink.user_timing", tracing::Level::TRACE) {
        return;
    }
    let now = cx.now_since(Some(cx.performance_window(performance)));
    let ago = |time: f64| base::numerics::saturated_cast::<i64, _>((now - time) * 1e6);
    let (start, duration) = cx.performance_entry_times(entry);
    let name = cx.performance_entry_name(entry).to_str();
    match cx.performance_entry_type(entry) {
        PerformanceEntryType::Mark => {
            tracing::trace!(target: "blink.user_timing", mark = &*name, ago_ns = ago(start));
        }
        PerformanceEntryType::Measure => tracing::trace!(
            target: "blink.user_timing",
            measure = &*name,
            start_ago_ns = ago(start),
            end_ago_ns = ago(start + duration),
        ),
        // Its times are marked as they happen (`Cx::mark_navigation`).
        PerformanceEntryType::Navigation => {}
    }
}

/// Each observer registered with `performance` that observes `entry`'s
/// type has it, and the task delivering them is queued if it isn't.
fn queue_to_observers<'gc>(
    cx: &ApiCx<'_, 'gc>,
    performance: Gc<'gc, EventTargetImpl>,
    entry: Gc<'gc, PerformanceEntry>,
) {
    let entry_type = cx.performance_entry_type(entry);
    let mut observed = false;
    for observer in cx.performance_observers(performance) {
        if cx.performance_observer_observes(observer, entry_type) {
            cx.queue_performance_observer_entry(performance, observer, entry);
            observed = true;
        }
    }
    if observed {
        cx.queue_performance_observer_task(performance);
    }
}

/// The task (`ElementTaskKind::PerformanceTimeline`): each observer with
/// entries to deliver, in the order it got its first (one emptied by
/// `takeRecords()` since is skipped), is called back with them as an entry
/// list in `startTime` order, itself, and the options:
/// `droppedEntriesCount` (0: machopo drops none) the first time after an
/// `observe()`. What a callback throws is reported, and the next called.
pub(crate) fn notify<H: ScriptHost>(host: &mut H, performance: &Persistent<EventTargetImpl>) {
    let observers: Vec<Persistent<PerformanceObserver>> = host.mutate(|cx| {
        cx.take_active_performance_observers(performance.get(cx.gc))
            .into_iter()
            .map(|observer| Persistent::new(cx.gc, observer))
            .collect()
    });
    for observer in observers {
        let call = host.mutate(|cx| {
            let observer = observer.get(cx.gc);
            let mut entries = cx.take_performance_observer_entries(observer);
            if entries.is_empty() {
                return None;
            }
            entries.sort_by(|&a, &b| start_time_of(cx, a).total_cmp(&start_time_of(cx, b)));
            let list = cx.create_performance_observer_entry_list(&entries);
            let dropped = cx
                .take_performance_observer_requires_dropped(observer)
                .then_some(0);
            Some((Persistent::new(cx.gc, list), dropped))
        });
        if let Some((list, dropped)) = call {
            host.call_performance_callback(&observer, &list, dropped);
        }
    }
}

/// `getEntries()` of an entry list.
pub(crate) fn list_entries<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceObserverEntryList>,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(cx.performance_observer_entry_list_entries(this))
}

pub(crate) fn list_entries_by_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceObserverEntryList>,
    entry_type: &str,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(filter(
        cx,
        cx.performance_observer_entry_list_entries(this),
        None,
        Some(entry_type),
    ))
}

pub(crate) fn list_entries_by_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceObserverEntryList>,
    name: &str,
    entry_type: Option<&str>,
) -> Result<Vec<Gc<'gc, PerformanceEntry>>, ApiError> {
    Ok(filter(
        cx,
        cx.performance_observer_entry_list_entries(this),
        Some(name),
        entry_type,
    ))
}

// --- Navigation Timing -----------------------------------------------------------------

/// Blink's `PerformanceResourceTiming` counts a response's head as this
/// many bytes, whatever it took, so as not to tell its size.
const HEADER_SIZE: u64 = 300;

/// `confidence.randomizedTriggerRate`: Chrome 152's.
pub const RANDOMIZED_TRIGGER_RATE: f64 = 0.4994798;

/// `performance`'s window's document's navigation entry, made and put in
/// the buffer the first time (when script first reaches `performance`),
/// named its URL, at 0, if its host gave the window a navigation timing.
fn ensure_navigation_entry<'gc>(
    cx: &ApiCx<'_, 'gc>,
    window: Gc<'gc, Window>,
    performance: Gc<'gc, EventTargetImpl>,
) {
    if cx.navigation_timing(window).is_none()
        || cx.set_performance_navigation_entry_made(performance)
    {
        return;
    }
    let url = cx
        .document_url(cx.window_document(window))
        .map(|url| url.to_str().into_owned())
        .unwrap_or_default();
    let entry = cx.create_performance_entry(
        window,
        PerformanceEntryType::Navigation,
        &url,
        0.0,
        0.0,
        None,
    );
    cx.add_performance_entry(performance, entry);
}

/// The document's `load` has ended (Navigation Timing's "queue the
/// navigation timing entry"): its observers get it, in a task, as Chrome
/// gives it them after `loadEventEnd`.
pub(crate) fn load_ended<H: ScriptHost>(host: &mut H, document: &NodeRoot) {
    host.mutate(|cx| {
        let Some(window) = cx.window_of(document.get(cx.gc)) else {
            return;
        };
        if cx.navigation_timing(window).is_none() {
            return;
        }
        let performance = cx.window_performance(window);
        ensure_navigation_entry(cx, window, performance);
        let entry = cx
            .performance_entries(performance)
            .into_iter()
            .find(|&entry| cx.performance_entry_type(entry) == PerformanceEntryType::Navigation);
        if let Some(entry) = entry {
            queue_to_observers(cx, performance, entry);
        }
    });
}

fn entry_timing<'gc>(cx: &ApiCx<'_, 'gc>, entry: Gc<'gc, PerformanceEntry>) -> NavigationTiming {
    cx.navigation_timing(cx.performance_entry_window(entry))
        .unwrap_or_default()
}

/// A time on the document's timeline (0 for none), coarsened.
fn relative(time: Option<f64>) -> f64 {
    time.map_or(0.0, |time| coarsen(time).max(0.0))
}

macro_rules! times {
    ($($(#[$meta:meta])* $name:ident => $field:ident;)*) => {$(
        $(#[$meta])*
        pub(crate) fn $name<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, PerformanceEntry>) -> Result<f64, ApiError> {
            Ok(relative(entry_timing(cx, this).$field))
        }
    )*};
}

times! {
    unload_event_start => unload_event_start;
    unload_event_end => unload_event_end;
    redirect_start => redirect_start;
    redirect_end => redirect_end;
    fetch_start => fetch_start;
    domain_lookup_start => domain_lookup_start;
    domain_lookup_end => domain_lookup_end;
    connect_start => connect_start;
    connect_end => connect_end;
    secure_connection_start => secure_connection_start;
    request_start => request_start;
    response_start => response_start;
    /// `finalResponseHeadersStart`: the final response's head (machopo has no
    /// interim ones), as `responseStart`.
    final_response_headers_start => response_start;
    response_end => response_end;
    dom_interactive => dom_interactive;
    dom_content_loaded_event_start => dom_content_loaded_event_start;
    dom_content_loaded_event_end => dom_content_loaded_event_end;
    dom_complete => dom_complete;
    load_event_start => load_event_start;
    load_event_end => load_event_end;
}

/// The attributes that are always 0 in machopo: no service worker, no interim
/// response, no prerendering, no Client Hints restart.
pub(crate) fn zero<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, PerformanceEntry>) -> Result<f64, ApiError> {
    Ok(0.0)
}

fn string<'gc>(text: impl Into<std::borrow::Cow<'static, str>>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh(text))
}

pub(crate) fn initiator_type<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string("navigation")
}

/// The attributes that are always the empty string in machopo: `deliveryType`
/// (not from a cache, not prefetched), and the service worker's.
pub(crate) fn empty<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string("")
}

pub(crate) fn next_hop_protocol<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string(entry_timing(cx, this).next_hop_protocol.clone())
}

/// A document is never render-blocking.
pub(crate) fn render_blocking_status<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string("non-blocking")
}

pub(crate) fn content_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string(entry_timing(cx, this).content_type.clone())
}

pub(crate) fn content_encoding<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string(entry_timing(cx, this).content_encoding.clone())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a document's bytes: far below 2^53"
)]
fn bytes(n: u64) -> f64 {
    n as f64
}

/// `transferSize`: the body's encoded bytes and Blink's fixed size of a
/// head, for a document that came over the network; 0 otherwise.
pub(crate) fn transfer_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<f64, ApiError> {
    let timing = entry_timing(cx, this);
    Ok(bytes(if timing.transferred {
        timing.encoded_body_size.saturating_add(HEADER_SIZE)
    } else {
        0
    }))
}

pub(crate) fn encoded_body_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<f64, ApiError> {
    Ok(bytes(entry_timing(cx, this).encoded_body_size))
}

pub(crate) fn decoded_body_size<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<f64, ApiError> {
    Ok(bytes(entry_timing(cx, this).decoded_body_size))
}

pub(crate) fn response_status<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<u32, ApiError> {
    Ok(entry_timing(cx, this).response_status)
}

/// `serverTiming`: none (machopo doesn't parse `Server-Timing`): an empty
/// frozen array.
pub(crate) fn server_timing<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, PerformanceEntry>,
) -> Result<Vec<f64>, ApiError> {
    Ok(Vec::new())
}

pub(crate) fn navigation_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<DomStr<'gc>, ApiError> {
    string(match entry_timing(cx, this).navigation_type {
        NavigationType::Navigate => "navigate",
        NavigationType::Reload => "reload",
        NavigationType::BackForward => "back_forward",
    })
}

pub(crate) fn redirect_count<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<u32, ApiError> {
    Ok(entry_timing(cx, this).redirect_count)
}

/// `notRestoredReasons`: `null` (machopo has no back-forward cache).
pub(crate) fn not_restored_reasons<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, PerformanceEntry>,
) -> Result<(), ApiError> {
    Ok(())
}

/// `confidence`: a new object each time, as in Chrome.
pub(crate) fn confidence<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceEntry>,
) -> Result<Gc<'gc, PerformanceTiming>, ApiError> {
    let high = entry_timing(cx, this).confidence_high;
    let window = cx.performance_entry_window(this);
    let performance = cx.window_performance(window);
    Ok(cx.performance_timing_of(performance, PerformanceTimingKind::Confidence(high)))
}

pub(crate) fn randomized_trigger_rate<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, PerformanceTiming>,
) -> Result<f64, ApiError> {
    Ok(RANDOMIZED_TRIGGER_RATE)
}

pub(crate) fn confidence_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceTiming>,
) -> Result<DomStr<'gc>, ApiError> {
    string(match cx.performance_timing_kind(this) {
        PerformanceTimingKind::Confidence(true) => "high",
        _ => "low",
    })
}

// --- `performance.timing` and `performance.navigation` ---------------------------------

pub(crate) fn timing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Gc<'gc, PerformanceTiming>, ApiError> {
    Ok(cx.performance_timing_of(this, PerformanceTimingKind::Timing))
}

pub(crate) fn navigation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Gc<'gc, PerformanceTiming>, ApiError> {
    Ok(cx.performance_timing_of(this, PerformanceTimingKind::Navigation))
}

/// `PerformanceTiming`'s attribute `name` of `window`'s document: whole
/// milliseconds since the Unix epoch, 0 for what hasn't happened (User
/// Timing's marks of those names read them too).
fn legacy_time<'gc>(cx: &ApiCx<'_, 'gc>, window: Gc<'gc, Window>, name: &str) -> u64 {
    let timing = cx.navigation_timing(window).unwrap_or_default();
    let time = match name {
        "navigationStart" => Some(0.0),
        "redirectStart" => timing.redirect_start,
        "redirectEnd" => timing.redirect_end,
        "fetchStart" => timing.fetch_start,
        "domainLookupStart" => timing.domain_lookup_start,
        "domainLookupEnd" => timing.domain_lookup_end,
        "connectStart" => timing.connect_start,
        "connectEnd" => timing.connect_end,
        "secureConnectionStart" => timing.secure_connection_start,
        "requestStart" => timing.request_start,
        "responseStart" => timing.response_start,
        "responseEnd" => timing.response_end,
        "domLoading" => timing.dom_loading,
        "domInteractive" => timing.dom_interactive,
        "domContentLoadedEventStart" => timing.dom_content_loaded_event_start,
        "domContentLoadedEventEnd" => timing.dom_content_loaded_event_end,
        "domComplete" => timing.dom_complete,
        "loadEventStart" => timing.load_event_start,
        "loadEventEnd" => timing.load_event_end,
        "unloadEventStart" => timing.unload_event_start,
        "unloadEventEnd" => timing.unload_event_end,
        _ => None,
    };
    time.map_or(0, |time| {
        base::numerics::saturated_cast::<u64, _>(
            (cx.window_unix_time_origin(window) + time).floor(),
        )
    })
}

/// `PerformanceTiming`'s attributes, by name.
pub(crate) fn legacy<'gc, const NAME: u8>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceTiming>,
) -> Result<f64, ApiError> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "milliseconds since the epoch: far below 2^53"
    )]
    Ok(legacy_time(
        cx,
        cx.performance_timing_window(this),
        TIMING_NAMES[usize::from(NAME)],
    ) as f64)
}

/// `performance.navigation.type`: Navigation Timing Level 1's numbers.
pub(crate) fn legacy_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceTiming>,
) -> Result<u32, ApiError> {
    let timing = cx
        .navigation_timing(cx.performance_timing_window(this))
        .unwrap_or_default();
    Ok(match timing.navigation_type {
        NavigationType::Navigate => 0,
        NavigationType::Reload => 1,
        NavigationType::BackForward => 2,
    })
}

pub(crate) fn legacy_redirect_count<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, PerformanceTiming>,
) -> Result<u32, ApiError> {
    Ok(cx
        .navigation_timing(cx.performance_timing_window(this))
        .unwrap_or_default()
        .redirect_count)
}
