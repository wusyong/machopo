//! A window's `performance` (Blink's `core/timing/performance.h`): an event
//! target of its own (`EventTargetImpl`'s `performance`), made the first
//! time the window's is read and kept by it; its realm is the window, on
//! whose timeline it is. It keeps its performance entry buffer (the marks,
//! the measures and the document's navigation entry, in the order they
//! came; one GC type for every kind, as Blink's `PerformanceEntry` is a
//! base class), the observers registered with it and those with entries to
//! deliver (Performance Timeline's), and its `timing` and `navigation`.
//! The window keeps its document's navigation timing (Navigation
//! Timing's, Blink's `DocumentLoadTiming`): the network's half its host
//! gives it as the document commits, the document's half marked as its
//! events are fired (`mark_navigation`). What they answer is
//! `dom_api`'s.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use crate::{Cx, DomString, EventTargetImpl, Window, sites};

/// An entry's `entryType`, which its interface goes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceEntryType {
    /// `PerformanceMark`.
    Mark,
    /// `PerformanceMeasure`.
    Measure,
    /// `PerformanceNavigationTiming`: a document's own, one in its
    /// window's buffer, whose times are its window's navigation timing's.
    Navigation,
}

impl PerformanceEntryType {
    /// Its `entryType`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mark => "mark",
            Self::Measure => "measure",
            Self::Navigation => "navigation",
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::Mark => 1,
            Self::Measure => 2,
            Self::Navigation => 4,
        }
    }

    fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Mark,
            2 => Self::Measure,
            _ => Self::Navigation,
        }
    }
}

/// How a document's navigation came about (`PerformanceNavigationTiming.
/// type`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NavigationType {
    #[default]
    Navigate,
    Reload,
    BackForward,
}

/// A document's navigation timing (Blink's `DocumentLoadTiming` and the
/// response's): times in milliseconds since its window's time origin,
/// none until they happen (or for what didn't: no redirect, no connection
/// opened). The host gives what came before the document (the network's
/// times, the response's sizes and kind); the document's own are marked
/// as they happen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavigationTiming {
    pub navigation_type: NavigationType,
    /// The redirects followed, when every one was of the document's
    /// origin (0 otherwise, as their times are none).
    pub redirect_count: u32,
    pub redirect_start: Option<f64>,
    pub redirect_end: Option<f64>,
    /// The `unload` of the document before it in its frame, if it was of
    /// its origin and no redirect of another came between (HTML's "unload
    /// timing info").
    pub unload_event_start: Option<f64>,
    pub unload_event_end: Option<f64>,
    pub fetch_start: Option<f64>,
    pub domain_lookup_start: Option<f64>,
    pub domain_lookup_end: Option<f64>,
    pub connect_start: Option<f64>,
    pub connect_end: Option<f64>,
    pub secure_connection_start: Option<f64>,
    pub request_start: Option<f64>,
    pub response_start: Option<f64>,
    pub response_end: Option<f64>,
    /// `http/1.1`, `h2`...; empty for none.
    pub next_hop_protocol: String,
    /// Over the network: the body's bytes, before and after
    /// `Content-Encoding` was undone.
    pub encoded_body_size: u64,
    pub decoded_body_size: u64,
    /// Whether it came over the network (its `transferSize` counts).
    pub transferred: bool,
    pub response_status: u32,
    pub content_type: String,
    pub content_encoding: String,
    pub dom_loading: Option<f64>,
    pub dom_interactive: Option<f64>,
    pub dom_content_loaded_event_start: Option<f64>,
    pub dom_content_loaded_event_end: Option<f64>,
    pub dom_complete: Option<f64>,
    pub load_event_start: Option<f64>,
    pub load_event_end: Option<f64>,
    /// `confidence.value`: `high`, or `low`.
    pub confidence_high: bool,
}

/// A moment of a document's own that its navigation timing marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationMilestone {
    DomInteractive,
    DomContentLoadedEventStart,
    DomContentLoadedEventEnd,
    DomComplete,
    LoadEventStart,
    LoadEventEnd,
}

impl NavigationMilestone {
    /// Its name in Navigation Timing (and Chrome's trace).
    pub fn name(self) -> &'static str {
        match self {
            Self::DomInteractive => "domInteractive",
            Self::DomContentLoadedEventStart => "domContentLoadedEventStart",
            Self::DomContentLoadedEventEnd => "domContentLoadedEventEnd",
            Self::DomComplete => "domComplete",
            Self::LoadEventStart => "loadEventStart",
            Self::LoadEventEnd => "loadEventEnd",
        }
    }
}

#[derive(GcObject)]
pub struct PerformanceData {
    window: Member<Window>,
    /// Its buffer, the oldest first.
    entries: Member<HeapVector<PerformanceEntry>>,
    /// The observers registered with it, in the order they first observed;
    /// those with entries to deliver, in the order each got its first
    /// since the last delivery (Blink's `active_observers_`).
    observers: Member<HeapVector<PerformanceObserver>>,
    active: Member<HeapVector<PerformanceObserver>>,
    /// Whether the task delivering its observers' entries is queued.
    task_queued: Cell<bool>,
    /// Its `timing` and `navigation`, once asked for.
    timing: Member<PerformanceTiming>,
    navigation: Member<PerformanceTiming>,
    /// Whether its buffer has its document's navigation entry.
    navigation_entry: Cell<bool>,
}

/// A performance entry. What it says never changes after it is made (a
/// navigation entry's times are its window's navigation timing's).
#[derive(GcObject)]
pub struct PerformanceEntry {
    /// Its relevant global: the window whose `performance` made it, or the
    /// realm's that constructed it.
    window: Member<Window>,
    entry_type: Cell<u8>,
    name: LocalExternal<DomString>,
    start_time: Cell<f64>,
    duration: Cell<f64>,
    /// A mark's or a measure's `detail`, serialized (none: `null`), and its
    /// deserialization, the engine's (the same object each time, made at
    /// the first read: `fresh`).
    detail: LocalExternal<Option<Vec<u8>>>,
    detail_value: JsEdge,
    fresh: Cell<bool>,
    wrapper: JsEdge,
}

/// `PerformanceObserver.observe()`'s way of asking, which can't change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceObserverMode {
    None,
    /// `entryTypes`.
    Multiple,
    /// `type`.
    Single,
}

#[derive(GcObject)]
pub struct PerformanceObserver {
    window: Member<Window>,
    /// Its callback, the engine's to set (`performance_observer_callback_edge`).
    callback: JsEdge,
    mode: Cell<u8>,
    /// The entry types it observes, a bit each.
    types: Cell<u8>,
    /// Whether its next callback has `droppedEntriesCount` (the first after
    /// an `observe()`).
    requires_dropped: Cell<bool>,
    /// Its buffer, as the entries came.
    buffer: Member<HeapVector<PerformanceEntry>>,
    wrapper: JsEdge,
}

#[derive(GcObject)]
pub struct PerformanceObserverEntryList {
    entries: Member<HeapVector<PerformanceEntry>>,
    wrapper: JsEdge,
}

/// Which of the legacy objects a `PerformanceTiming` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformanceTimingKind {
    /// `performance.timing`.
    Timing,
    /// `performance.navigation`.
    Navigation,
    /// A navigation entry's `confidence`, high or not.
    Confidence(bool),
}

#[derive(GcObject)]
pub struct PerformanceTiming {
    window: Member<Window>,
    kind: Cell<u8>,
    wrapper: JsEdge,
}

impl PerformanceTimingKind {
    fn bits(self) -> u8 {
        match self {
            Self::Timing => 0,
            Self::Navigation => 1,
            Self::Confidence(true) => 2,
            Self::Confidence(false) => 3,
        }
    }

    fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Timing,
            1 => Self::Navigation,
            2 => Self::Confidence(true),
            _ => Self::Confidence(false),
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// `window`'s `performance`, the same each time.
    pub fn window_performance(&self, window: Gc<'gc, Window>) -> Gc<'gc, EventTargetImpl> {
        if let Some(performance) = window.performance.get(self.gc) {
            return performance;
        }
        let target = self.create_event_target();
        let data = self.gc.alloc_in(
            &sites::OTHER,
            PerformanceDataInit {
                window: Some(window),
                entries: None,
                observers: None,
                active: None,
                task_queued: Cell::new(false),
                timing: None,
                navigation: None,
                navigation_entry: Cell::new(false),
            },
        );
        target.performance.set(self.gc, Some(data));
        window.performance.set(self.gc, Some(target));
        target
    }

    pub fn is_performance(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        target.performance.get(self.gc).is_some()
    }

    fn performance_data(&self, performance: Gc<'gc, EventTargetImpl>) -> Gc<'gc, PerformanceData> {
        performance.performance.get(self.gc).expect("a performance")
    }

    pub fn performance_window(&self, performance: Gc<'gc, EventTargetImpl>) -> Gc<'gc, Window> {
        self.performance_data(performance)
            .window
            .get(self.gc)
            .expect("a performance's window")
    }

    /// A new entry of `window`'s, in no buffer.
    pub fn create_performance_entry(
        &self,
        window: Gc<'gc, Window>,
        entry_type: PerformanceEntryType,
        name: &str,
        start_time: f64,
        duration: f64,
        detail: Option<Vec<u8>>,
    ) -> Gc<'gc, PerformanceEntry> {
        self.gc.alloc_in(
            &sites::OTHER,
            PerformanceEntryInit {
                window: Some(window),
                entry_type: Cell::new(entry_type.bit()),
                name: LocalExternal::new(DomString::new(name)),
                start_time: Cell::new(start_time),
                duration: Cell::new(duration),
                detail: LocalExternal::new(detail),
                detail_value: (),
                fresh: Cell::new(false),
                wrapper: (),
            },
        )
    }

    pub fn performance_entry_type(&self, entry: Gc<'gc, PerformanceEntry>) -> PerformanceEntryType {
        PerformanceEntryType::from_bits(entry.entry_type.get())
    }

    pub fn performance_entry_name(&self, entry: Gc<'gc, PerformanceEntry>) -> &'gc DomString {
        entry.into_ref().name.get()
    }

    pub fn performance_entry_times(&self, entry: Gc<'gc, PerformanceEntry>) -> (f64, f64) {
        (entry.start_time.get(), entry.duration.get())
    }

    pub fn performance_entry_window(&self, entry: Gc<'gc, PerformanceEntry>) -> Gc<'gc, Window> {
        entry.window.get(self.gc).expect("an entry's window")
    }

    /// A mark's or a measure's `detail` serialized (none: `null`).
    pub fn performance_entry_detail(&self, entry: Gc<'gc, PerformanceEntry>) -> Option<&'gc [u8]> {
        entry.into_ref().detail.get().as_deref()
    }

    /// The deserialized `detail`'s edge, and whether the engine made it.
    pub fn performance_entry_detail_edge(
        &self,
        entry: Gc<'gc, PerformanceEntry>,
    ) -> (&'gc JsEdge, bool) {
        (&entry.into_ref().detail_value, entry.fresh.get())
    }

    pub fn set_performance_entry_detail_fresh(&self, entry: Gc<'gc, PerformanceEntry>) {
        entry.fresh.set(true);
    }

    /// `performance`'s buffer, the oldest first.
    pub fn performance_entries(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
    ) -> Vec<Gc<'gc, PerformanceEntry>> {
        let Some(entries) = self.performance_data(performance).entries.get(self.gc) else {
            return Vec::new();
        };
        (0..entries.len())
            .filter_map(|at| entries.get(self.gc, at))
            .collect()
    }

    /// `entry` at the end of `performance`'s buffer.
    pub fn add_performance_entry(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
        entry: Gc<'gc, PerformanceEntry>,
    ) {
        let data = self.performance_data(performance);
        let entries = match data.entries.get(self.gc) {
            Some(entries) => entries,
            None => {
                let entries = HeapVector::new(self.gc, 4);
                data.entries.set(self.gc, Some(entries));
                entries
            }
        };
        entries.push(self.gc, entry);
    }

    /// The buffer's entries of `entry_type` (named `name`, if given) gone.
    pub fn clear_performance_entries(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
        entry_type: PerformanceEntryType,
        name: Option<&str>,
    ) {
        let Some(entries) = self.performance_data(performance).entries.get(self.gc) else {
            return;
        };
        let mut at = 0;
        while let Some(entry) = entries.get(self.gc, at) {
            if self.performance_entry_type(entry) == entry_type
                && name.is_none_or(|name| *self.performance_entry_name(entry).to_str() == *name)
            {
                entries.remove(self.gc, at);
            } else {
                at = at.saturating_add(1);
            }
        }
    }

    /// Whether `performance`'s buffer has its document's navigation entry
    /// yet; it has from now on.
    pub fn set_performance_navigation_entry_made(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
    ) -> bool {
        self.performance_data(performance)
            .navigation_entry
            .replace(true)
    }

    /// `performance`'s `timing` or `navigation` (the same each time), or a
    /// new `confidence`.
    pub fn performance_timing_of(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
        kind: PerformanceTimingKind,
    ) -> Gc<'gc, PerformanceTiming> {
        let data = self.performance_data(performance);
        let window = data.window.get(self.gc);
        let make = || {
            self.gc.alloc_in(
                &sites::OTHER,
                PerformanceTimingInit {
                    window,
                    kind: Cell::new(kind.bits()),
                    wrapper: (),
                },
            )
        };
        let kept = match kind {
            PerformanceTimingKind::Timing => &data.into_ref().timing,
            PerformanceTimingKind::Navigation => &data.into_ref().navigation,
            PerformanceTimingKind::Confidence(_) => return make(),
        };
        if let Some(timing) = kept.get(self.gc) {
            return timing;
        }
        let timing = make();
        kept.set(self.gc, Some(timing));
        timing
    }

    pub fn performance_timing_kind(
        &self,
        timing: Gc<'gc, PerformanceTiming>,
    ) -> PerformanceTimingKind {
        PerformanceTimingKind::from_bits(timing.kind.get())
    }

    pub fn performance_timing_window(&self, timing: Gc<'gc, PerformanceTiming>) -> Gc<'gc, Window> {
        timing.window.get(self.gc).expect("a timing's window")
    }

    /// `window`'s document's navigation timing is `timing` (its host's, as
    /// the document commits).
    pub fn set_navigation_timing(&self, window: Gc<'gc, Window>, timing: NavigationTiming) {
        window.navigation_timing.set(Some(Box::new(timing)));
    }

    /// `window`'s document's navigation timing, if its host gave one.
    pub fn navigation_timing(&self, window: Gc<'gc, Window>) -> Option<NavigationTiming> {
        let cell = &*window.into_ref().navigation_timing;
        let timing = cell.take();
        let copy = timing.as_deref().cloned();
        cell.set(timing);
        copy
    }

    /// `window`'s document reached `milestone` now (if its host gave it a
    /// navigation timing; the first time only).
    pub fn mark_navigation(&self, window: Gc<'gc, Window>, milestone: NavigationMilestone) {
        let cell = &*window.into_ref().navigation_timing;
        let Some(mut timing) = cell.take() else {
            return;
        };
        let now = self.now_since(Some(window));
        // In machopo's trace too, a mark named as Chrome's (its first time,
        // which the timing keeps).
        let trace = |name: &'static str| {
            tracing::trace!(target: "blink.user_timing", mark = name, ago_ns = 0i64);
        };
        let field = match milestone {
            NavigationMilestone::DomInteractive => &mut timing.dom_interactive,
            NavigationMilestone::DomContentLoadedEventStart => {
                &mut timing.dom_content_loaded_event_start
            }
            NavigationMilestone::DomContentLoadedEventEnd => {
                &mut timing.dom_content_loaded_event_end
            }
            NavigationMilestone::DomComplete => &mut timing.dom_complete,
            NavigationMilestone::LoadEventStart => &mut timing.load_event_start,
            NavigationMilestone::LoadEventEnd => &mut timing.load_event_end,
        };
        if field.is_none() {
            trace(milestone.name());
        }
        field.get_or_insert(now);
        cell.set(Some(timing));
    }

    /// A new observer of `window`'s.
    pub fn create_performance_observer(
        &self,
        window: Gc<'gc, Window>,
    ) -> Gc<'gc, PerformanceObserver> {
        self.gc.alloc_in(
            &sites::OTHER,
            PerformanceObserverInit {
                window: Some(window),
                callback: (),
                mode: Cell::new(0),
                types: Cell::new(0),
                requires_dropped: Cell::new(false),
                buffer: None,
                wrapper: (),
            },
        )
    }

    pub fn performance_observer_window(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
    ) -> Gc<'gc, Window> {
        observer.window.get(self.gc).expect("an observer's window")
    }

    pub fn performance_observer_callback_edge(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
    ) -> &'gc JsEdge {
        &observer.into_ref().callback
    }

    pub fn performance_observer_mode(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
    ) -> PerformanceObserverMode {
        match observer.mode.get() {
            0 => PerformanceObserverMode::None,
            1 => PerformanceObserverMode::Multiple,
            _ => PerformanceObserverMode::Single,
        }
    }

    pub fn set_performance_observer_mode(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
        mode: PerformanceObserverMode,
    ) {
        observer.mode.set(match mode {
            PerformanceObserverMode::None => 0,
            PerformanceObserverMode::Multiple => 1,
            PerformanceObserverMode::Single => 2,
        });
    }

    /// `observer` registered with `performance` (if it isn't), observing
    /// `types` too (in `Multiple` mode, only them); its next callback has
    /// `droppedEntriesCount`.
    pub fn observe_performance(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
        observer: Gc<'gc, PerformanceObserver>,
        types: &[PerformanceEntryType],
    ) {
        let bits = types
            .iter()
            .fold(0, |bits, entry_type| bits | entry_type.bit());
        if self.performance_observer_mode(observer) == PerformanceObserverMode::Multiple {
            observer.types.set(bits);
        } else {
            observer.types.set(observer.types.get() | bits);
        }
        observer.requires_dropped.set(true);
        let data = self.performance_data(performance);
        let observers = match data.observers.get(self.gc) {
            Some(observers) => observers,
            None => {
                let observers = HeapVector::new(self.gc, 1);
                data.observers.set(self.gc, Some(observers));
                observers
            }
        };
        if !contains(self, observers, observer) {
            observers.push(self.gc, observer);
        }
    }

    pub fn performance_observer_observes(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
        entry_type: PerformanceEntryType,
    ) -> bool {
        observer.types.get() & entry_type.bit() != 0
    }

    /// The observers registered with `performance`.
    pub fn performance_observers(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
    ) -> Vec<Gc<'gc, PerformanceObserver>> {
        let Some(observers) = self.performance_data(performance).observers.get(self.gc) else {
            return Vec::new();
        };
        (0..observers.len())
            .filter_map(|at| observers.get(self.gc, at))
            .collect()
    }

    /// `disconnect()`: `observer` registered no more, its buffer emptied.
    pub fn disconnect_performance_observer(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
        observer: Gc<'gc, PerformanceObserver>,
    ) {
        let data = self.performance_data(performance);
        for list in [data.observers.get(self.gc), data.active.get(self.gc)]
            .into_iter()
            .flatten()
        {
            if let Some(at) = (0..list.len()).find(|&at| {
                list.get(self.gc, at)
                    .is_some_and(|other| Gc::ptr_eq(other, observer))
            }) {
                list.remove(self.gc, at);
            }
        }
        if let Some(buffer) = observer.buffer.get(self.gc) {
            buffer.clear(self.gc);
        }
        observer.types.set(0);
    }

    /// `entry` in `observer`'s buffer; `observer` among `performance`'s
    /// active ones if it isn't.
    pub fn queue_performance_observer_entry(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
        observer: Gc<'gc, PerformanceObserver>,
        entry: Gc<'gc, PerformanceEntry>,
    ) {
        let buffer = match observer.buffer.get(self.gc) {
            Some(buffer) => buffer,
            None => {
                let buffer = HeapVector::new(self.gc, 1);
                observer.buffer.set(self.gc, Some(buffer));
                buffer
            }
        };
        buffer.push(self.gc, entry);
        let data = self.performance_data(performance);
        let active = match data.active.get(self.gc) {
            Some(active) => active,
            None => {
                let active = HeapVector::new(self.gc, 1);
                data.active.set(self.gc, Some(active));
                active
            }
        };
        if !contains(self, active, observer) {
            active.push(self.gc, observer);
        }
    }

    /// `observer`'s buffer, taken (`takeRecords()`, a delivery).
    pub fn take_performance_observer_entries(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
    ) -> Vec<Gc<'gc, PerformanceEntry>> {
        let Some(buffer) = observer.buffer.get(self.gc) else {
            return Vec::new();
        };
        let entries = (0..buffer.len())
            .filter_map(|at| buffer.get(self.gc, at))
            .collect();
        buffer.clear(self.gc);
        entries
    }

    /// Whether `observer`'s callback is to have `droppedEntriesCount`; it
    /// isn't from now on.
    pub fn take_performance_observer_requires_dropped(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
    ) -> bool {
        observer.requires_dropped.replace(false)
    }

    /// `performance`'s observers with entries to deliver, taken; the task
    /// is no longer queued.
    pub fn take_active_performance_observers(
        &self,
        performance: Gc<'gc, EventTargetImpl>,
    ) -> Vec<Gc<'gc, PerformanceObserver>> {
        let data = self.performance_data(performance);
        data.task_queued.set(false);
        let Some(active) = data.active.get(self.gc) else {
            return Vec::new();
        };
        let observers = (0..active.len())
            .filter_map(|at| active.get(self.gc, at))
            .collect();
        active.clear(self.gc);
        observers
    }

    /// Queues the task delivering `performance`'s observers' entries (a
    /// window element task of `performance`'s), unless it is queued.
    pub fn queue_performance_observer_task(&self, performance: Gc<'gc, EventTargetImpl>) {
        let data = self.performance_data(performance);
        if data.task_queued.replace(true) {
            return;
        }
        if let Some(window) = data.window.get(self.gc) {
            self.push_target_task(window, performance, crate::window::PERFORMANCE_TIMELINE, 0);
        }
    }

    /// A new entry list of `entries`.
    pub fn create_performance_observer_entry_list(
        &self,
        entries: &[Gc<'gc, PerformanceEntry>],
    ) -> Gc<'gc, PerformanceObserverEntryList> {
        let list = HeapVector::new(self.gc, entries.len().max(1));
        for &entry in entries {
            list.push(self.gc, entry);
        }
        self.gc.alloc_in(
            &sites::OTHER,
            PerformanceObserverEntryListInit {
                entries: Some(list),
                wrapper: (),
            },
        )
    }

    pub fn performance_observer_entry_list_entries(
        &self,
        list: Gc<'gc, PerformanceObserverEntryList>,
    ) -> Vec<Gc<'gc, PerformanceEntry>> {
        let Some(entries) = list.entries.get(self.gc) else {
            return Vec::new();
        };
        (0..entries.len())
            .filter_map(|at| entries.get(self.gc, at))
            .collect()
    }

    pub(crate) fn performance_entry_wrapper_edge(
        &self,
        entry: Gc<'gc, PerformanceEntry>,
    ) -> &'gc JsEdge {
        &entry.into_ref().wrapper
    }

    pub(crate) fn performance_observer_wrapper_edge(
        &self,
        observer: Gc<'gc, PerformanceObserver>,
    ) -> &'gc JsEdge {
        &observer.into_ref().wrapper
    }

    pub(crate) fn entry_list_wrapper_edge(
        &self,
        list: Gc<'gc, PerformanceObserverEntryList>,
    ) -> &'gc JsEdge {
        &list.into_ref().wrapper
    }

    pub(crate) fn performance_timing_wrapper_edge(
        &self,
        timing: Gc<'gc, PerformanceTiming>,
    ) -> &'gc JsEdge {
        &timing.into_ref().wrapper
    }
}

fn contains<'gc>(
    cx: &Cx<'_, 'gc>,
    list: Gc<'gc, HeapVector<PerformanceObserver>>,
    observer: Gc<'gc, PerformanceObserver>,
) -> bool {
    (0..list.len()).any(|at| {
        list.get(cx.gc, at)
            .is_some_and(|other| Gc::ptr_eq(other, observer))
    })
}
