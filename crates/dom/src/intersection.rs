//! Intersection observers (Intersection Observer's `IntersectionObserver`,
//! v2 included; Blink's `core/intersection_observer/`): the GC types, and
//! who keeps whom. Where the targets are is the renderer's (the rendering
//! update computes the intersections), when the entries are made and the
//! callbacks run the engine's (`dom_api`).
//!
//! - **Who keeps whom**: an element keeps its observations (its
//!   `ElementData`'s list), an observation its observer; an observer keeps
//!   its observations only weakly, so it doesn't keep the elements it
//!   observes alive, and its explicit root only weakly (Blink's
//!   `ElementIntersectionObserverData` on the element, and
//!   `IntersectionObserver::root_`, a `WeakMember`).
//! - **A window's observers** (Blink's `IntersectionObserverController`):
//!   every observer made for it, weakly, in the order they were made, for
//!   the rendering update to find them; and those with entries queued
//!   since its task last ran, strongly, in the order they got their first,
//!   until the task delivers them.

use std::cell::Cell;

use elise::{External, Gc, GcObject, HeapVector, JsEdge, Member, WeakMember};
use gfx::{Margins, RectF};

use crate::{Cx, Node, NodeRef, Window, sites};

/// What an observer was made with (`IntersectionObserverInit`, parsed):
/// fixed for its life.
#[derive(Clone, Debug, PartialEq)]
pub struct IntersectionObserverOptions {
    pub root_margin: Margins,
    pub scroll_margin: Margins,
    /// Sorted, never empty.
    pub thresholds: Vec<f64>,
    /// `delay`, in microseconds (Blink's `base::TimeDelta`): the least
    /// time between two computations of an observation; 0 or less for
    /// none.
    pub delay_micros: i64,
    /// `trackVisibility`: its entries say whether the target is visible.
    pub track_visibility: bool,
}

/// `new IntersectionObserver(callback, options)`.
#[derive(GcObject)]
pub struct IntersectionObserver {
    callback: JsEdge,
    /// Its observations, weakly: the elements keep them.
    observations: Member<HeapVector<ObservationRef>>,
    /// Its explicit root, an element or a document, weakly; whether it was
    /// made with one (one gone since leaves it none to compute against,
    /// not the implicit root).
    root: WeakMember<Node>,
    explicit_root: Cell<bool>,
    options: External<Box<IntersectionObserverOptions>>,
    /// Its entry queue (the spec's `[[QueuedEntries]]`).
    entries: Member<HeapVector<IntersectionObserverEntry>>,
    /// Whether it is on its window's list of observers waiting for the
    /// task.
    pending: Cell<bool>,
    /// Whether it is a document's lazy load observer (no script's: the
    /// engine resumes its targets' loads in its stead).
    lazy_load: Cell<bool>,
    wrapper: JsEdge,
}

/// An observation held weakly, on its observer's list.
#[derive(GcObject)]
pub struct ObservationRef {
    observation: WeakMember<IntersectionObservation>,
}

/// An observer held weakly, on its window's list.
#[derive(GcObject)]
pub struct ObserverRef {
    observer: WeakMember<IntersectionObserver>,
}

/// An observation (the spec's `IntersectionObserverRegistration`, Blink's
/// `IntersectionObservation`): on its target's list, which keeps it.
#[derive(GcObject)]
pub struct IntersectionObservation {
    observer: Member<IntersectionObserver>,
    /// The element it is on, which keeps it: the edge back only makes a
    /// cycle the GC collects with the element.
    target: Member<Node>,
    /// The threshold index last computed (the spec's
    /// `previousThresholdIndex`); -1 until the first, so the first
    /// computation always queues an entry.
    previous_threshold_index: Cell<i64>,
    /// The spec's `previousIsIntersecting`.
    previous_is_intersecting: Cell<bool>,
    /// The `isVisible` last computed (Blink's `last_is_visible_`).
    previous_is_visible: Cell<bool>,
    /// When it was last computed, on the event loop's clock (Blink's
    /// `last_run_time_`), NaN before the first: its observer's `delay`
    /// counts from there.
    last_run: Cell<f64>,
}

/// An `IntersectionObserverEntry`: fixed when it is made.
#[derive(GcObject)]
pub struct IntersectionObserverEntry {
    target: Member<Node>,
    /// None for `null`: a target in a document not same origin-domain with
    /// the implicit root's.
    root_bounds: Member<crate::DomRect>,
    bounding_client_rect: Member<crate::DomRect>,
    intersection_rect: Member<crate::DomRect>,
    time: Cell<f64>,
    is_intersecting: Cell<bool>,
    is_visible: Cell<bool>,
    intersection_ratio: Cell<f64>,
    wrapper: JsEdge,
}

/// An observer's root as the rendering update reads it.
#[derive(Clone, Copy, Debug)]
pub enum IntersectionObserverRoot<'gc> {
    /// The top-level document's viewport.
    Implicit,
    /// An element or a document.
    Node(NodeRef<'gc>),
    /// The explicit root it was made with is gone: nothing to compute.
    Gone,
}

/// An observation as the rendering update reads it: its target, and what
/// was last computed.
#[derive(Clone, Copy, Debug)]
pub struct IntersectionObservationState<'gc> {
    pub target: NodeRef<'gc>,
    pub previous_threshold_index: Option<usize>,
    pub previous_is_intersecting: bool,
    pub previous_is_visible: bool,
    pub last_run: Option<f64>,
}

/// What a new entry is made with (the spec's "queue an
/// `IntersectionObserverEntry`"'s arguments): rects in their documents'
/// viewports, CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntersectionEntryValues {
    pub time: f64,
    pub root_bounds: Option<RectF>,
    pub bounding_client_rect: RectF,
    pub intersection_rect: RectF,
    pub is_intersecting: bool,
    pub is_visible: bool,
    pub intersection_ratio: f64,
}

impl<'gc> Cx<'_, 'gc> {
    /// `new IntersectionObserver(callback, options)` for `window`, with
    /// `root` (none for the implicit root): one of the window's observers.
    /// Its callback is the engine's to set (`intersection_callback_edge`).
    pub fn create_intersection_observer(
        &self,
        window: Gc<'gc, Window>,
        root: Option<NodeRef<'gc>>,
        options: IntersectionObserverOptions,
    ) -> Gc<'gc, IntersectionObserver> {
        let observer = self.gc.alloc_in(
            &sites::OTHER,
            IntersectionObserverInit {
                callback: (),
                observations: None,
                root,
                explicit_root: Cell::new(root.is_some()),
                options: External::new(Box::new(options)),
                entries: None,
                pending: Cell::new(false),
                lazy_load: Cell::new(false),
                wrapper: (),
            },
        );
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            ObserverRefInit {
                observer: Some(observer),
            },
        );
        let list = match window.intersection_observers.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                window.intersection_observers.set(self.gc, Some(list));
                list
            }
        };
        prune(self, list, |entry: Gc<'gc, ObserverRef>| {
            entry.observer.get(self.gc).is_some()
        });
        list.push(self.gc, entry);
        self.intersection_changed();
        observer
    }

    /// `document`'s lazy load observer (Blink's `LazyLoadImageObserver`,
    /// made at the first lazy `<img>`): Chrome's distances (its fast
    /// network's), 1250px up and down and half that sideways, from the
    /// viewport and from each scroll container's scrollport; any
    /// intersection at all. Chrome gives them as a scroll margin alone,
    /// which its viewport takes too; `paint::intersections` grows only the
    /// scroll containers' between the target and the root by it (WPT's
    /// `scroll-margin*.html`), so the viewport's is the root margin. None
    /// without a window.
    pub(crate) fn lazy_load_observer(
        &self,
        document: NodeRef<'gc>,
    ) -> Option<Gc<'gc, IntersectionObserver>> {
        let window = self.window_of(document)?;
        let data = self.document_data(document)?;
        if let Some(observer) = data.lazy_load_observer.get(self.gc) {
            return Some(observer);
        }
        let (vertical, horizontal) = (gfx::MarginLength::Px(1250.0), gfx::MarginLength::Px(625.0));
        let observer = self.create_intersection_observer(
            window,
            None,
            IntersectionObserverOptions {
                root_margin: [vertical, horizontal, vertical, horizontal],
                scroll_margin: [vertical, horizontal, vertical, horizontal],
                thresholds: vec![f64::from(f32::MIN_POSITIVE)],
                delay_micros: 0,
                track_visibility: false,
            },
        );
        observer.lazy_load.set(true);
        data.lazy_load_observer.set(self.gc, Some(observer));
        Some(observer)
    }

    /// Whether `observer` is a document's lazy load observer.
    pub fn is_lazy_load_observer(&self, observer: Gc<'gc, IntersectionObserver>) -> bool {
        observer.lazy_load.get()
    }

    /// `observer`'s reference to its callback, which only the engine reads
    /// and writes.
    pub fn intersection_callback_edge(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
    ) -> &'gc JsEdge {
        &observer.into_ref().callback
    }

    pub(crate) fn intersection_observer_wrapper_edge(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
    ) -> &'gc JsEdge {
        &observer.into_ref().wrapper
    }

    pub(crate) fn intersection_entry_wrapper_edge(
        &self,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> &'gc JsEdge {
        &entry.into_ref().wrapper
    }

    /// The observations and observers changed: the rendering update
    /// computes them again (`intersection_version`).
    fn intersection_changed(&self) {
        let version = &self.changes.intersection_version;
        version.set(version.get().wrapping_add(1));
    }

    /// How many times observers were made, or targets observed or
    /// unobserved: what the rendering update computed last holds while it
    /// is the same and nothing moved.
    pub fn intersection_version(&self) -> u64 {
        self.changes.intersection_version.get()
    }

    fn element_observations(
        &self,
        target: NodeRef<'gc>,
        make: bool,
    ) -> Option<Gc<'gc, HeapVector<IntersectionObservation>>> {
        if let Some(list) = self
            .existing_element_data(target)
            .and_then(|data| data.intersections.get(self.gc))
        {
            return Some(list);
        }
        if !make {
            return None;
        }
        let list = HeapVector::new(self.gc, 1);
        self.element_data(target)
            .intersections
            .set(self.gc, Some(list));
        Some(list)
    }

    /// `observer`'s observation of `target`, if there is one.
    fn observation_of(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
        target: NodeRef<'gc>,
    ) -> Option<Gc<'gc, IntersectionObservation>> {
        let list = self.element_observations(target, false)?;
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .find(|observation| {
                observation
                    .observer
                    .get(self.gc)
                    .is_some_and(|of| Gc::ptr_eq(of, observer))
            })
    }

    /// `observe(target)`: an observation of `target`, unless there is one.
    pub fn observe_intersection(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
        target: NodeRef<'gc>,
    ) {
        if self.kind(target) != crate::NodeKind::Element
            || self.observation_of(observer, target).is_some()
        {
            return;
        }
        let observation = self.gc.alloc_in(
            &sites::OTHER,
            IntersectionObservationInit {
                observer: Some(observer),
                target: Some(target),
                previous_threshold_index: Cell::new(-1),
                previous_is_intersecting: Cell::new(false),
                previous_is_visible: Cell::new(false),
                last_run: Cell::new(f64::NAN),
            },
        );
        self.element_observations(target, true)
            .expect("made")
            .push(self.gc, observation);
        let reference = self.gc.alloc_in(
            &sites::OTHER,
            ObservationRefInit {
                observation: Some(observation),
            },
        );
        let list = match observer.observations.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                observer.observations.set(self.gc, Some(list));
                list
            }
        };
        prune(self, list, |reference: Gc<'gc, ObservationRef>| {
            reference
                .observation
                .get(self.gc)
                .is_some_and(|observation| observation.observer.get(self.gc).is_some())
        });
        list.push(self.gc, reference);
        self.intersection_changed();
    }

    /// `unobserve(target)`: `observer`'s observation of it taken off.
    pub fn unobserve_intersection(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
        target: NodeRef<'gc>,
    ) {
        let Some(list) = self.element_observations(target, false) else {
            return;
        };
        let mut index = 0;
        while let Some(observation) = list.get(self.gc, index) {
            if observation
                .observer
                .get(self.gc)
                .is_some_and(|of| Gc::ptr_eq(of, observer))
            {
                list.remove(self.gc, index);
                // Off its observer's list too: it reads as dead there.
                observation.observer.set(self.gc, None);
                self.intersection_changed();
            } else {
                index = index.saturating_add(1);
            }
        }
    }

    /// `disconnect()`: every observation of `observer`'s taken off.
    pub fn disconnect_intersection_observer(&self, observer: Gc<'gc, IntersectionObserver>) {
        for state in self.intersection_observations(observer) {
            self.unobserve_intersection(observer, state.target);
        }
        if let Some(list) = observer.observations.get(self.gc) {
            list.clear(self.gc);
        }
    }

    /// `observer`'s observations (those alive, on their targets still), in
    /// the order they were made.
    pub fn intersection_observations(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
    ) -> Vec<IntersectionObservationState<'gc>> {
        let Some(list) = observer.observations.get(self.gc) else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.observation.get(self.gc))
            .filter(|observation| observation.observer.get(self.gc).is_some())
            .filter_map(|observation| {
                let index = observation.previous_threshold_index.get();
                let last_run = observation.last_run.get();
                Some(IntersectionObservationState {
                    target: observation.target.get(self.gc)?,
                    previous_threshold_index: usize::try_from(index).ok(),
                    previous_is_intersecting: observation.previous_is_intersecting.get(),
                    previous_is_visible: observation.previous_is_visible.get(),
                    last_run: (!last_run.is_nan()).then_some(last_run),
                })
            })
            .collect()
    }

    /// `window`'s observers, alive, in the order they were made.
    pub fn window_intersection_observers(
        &self,
        window: Gc<'gc, Window>,
    ) -> Vec<Gc<'gc, IntersectionObserver>> {
        let Some(list) = window.intersection_observers.get(self.gc) else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.observer.get(self.gc))
            .collect()
    }

    /// `observer`'s root.
    pub fn intersection_observer_root(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
    ) -> IntersectionObserverRoot<'gc> {
        match observer.root.get(self.gc) {
            Some(root) => IntersectionObserverRoot::Node(root),
            None if observer.explicit_root.get() => IntersectionObserverRoot::Gone,
            None => IntersectionObserverRoot::Implicit,
        }
    }

    pub fn intersection_observer_options(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
    ) -> &'gc IntersectionObserverOptions {
        &observer.into_ref().options
    }

    /// `observer`'s observation of `target` was computed at `now`.
    pub fn set_intersection_observation_run(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
        target: NodeRef<'gc>,
        now: f64,
    ) {
        if let Some(observation) = self.observation_of(observer, target) {
            observation.last_run.set(now);
        }
    }

    /// What `observer`'s observation of `target` came out as: its threshold
    /// index, whether it intersects and is visible.
    pub fn set_intersection_observation_state(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
        target: NodeRef<'gc>,
        index: usize,
        is_intersecting: bool,
        is_visible: bool,
    ) {
        if let Some(observation) = self.observation_of(observer, target) {
            observation
                .previous_threshold_index
                .set(i64::try_from(index).unwrap_or(i64::MAX));
            observation.previous_is_intersecting.set(is_intersecting);
            observation.previous_is_visible.set(is_visible);
        }
    }

    /// A new entry of `target`'s.
    pub fn create_intersection_entry(
        &self,
        target: NodeRef<'gc>,
        values: IntersectionEntryValues,
    ) -> Gc<'gc, IntersectionObserverEntry> {
        let root_bounds = values.root_bounds.map(|rect| self.new_read_only_rect(rect));
        let bounding_client_rect = self.new_read_only_rect(values.bounding_client_rect);
        let intersection_rect = self.new_read_only_rect(values.intersection_rect);
        self.gc.alloc_in(
            &sites::OTHER,
            IntersectionObserverEntryInit {
                target: Some(target),
                root_bounds,
                bounding_client_rect: Some(bounding_client_rect),
                intersection_rect: Some(intersection_rect),
                time: Cell::new(values.time),
                is_intersecting: Cell::new(values.is_intersecting),
                is_visible: Cell::new(values.is_visible),
                intersection_ratio: Cell::new(values.intersection_ratio),
                wrapper: (),
            },
        )
    }

    /// `entry` queued on `observer`, which goes on `window`'s list of
    /// observers waiting for the task if it isn't: whether that list was
    /// empty (the task is to be queued).
    pub fn queue_intersection_entry(
        &self,
        window: Gc<'gc, Window>,
        observer: Gc<'gc, IntersectionObserver>,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> bool {
        let entries = match observer.entries.get(self.gc) {
            Some(entries) => entries,
            None => {
                let entries = HeapVector::new(self.gc, 1);
                observer.entries.set(self.gc, Some(entries));
                entries
            }
        };
        entries.push(self.gc, entry);
        if observer.pending.replace(true) {
            return false;
        }
        let pending = match window.pending_intersection_observers.get(self.gc) {
            Some(pending) => pending,
            None => {
                let pending = HeapVector::new(self.gc, 1);
                window
                    .pending_intersection_observers
                    .set(self.gc, Some(pending));
                pending
            }
        };
        pending.push(self.gc, observer);
        pending.len() == 1
    }

    /// `window`'s observers waiting for the task, taken, in the order they
    /// got their first entry.
    pub fn take_pending_intersection_observers(
        &self,
        window: Gc<'gc, Window>,
    ) -> Vec<Gc<'gc, IntersectionObserver>> {
        let Some(pending) = window.pending_intersection_observers.get(self.gc) else {
            return Vec::new();
        };
        let taken: Vec<_> = (0..pending.len())
            .filter_map(|index| pending.get(self.gc, index))
            .collect();
        pending.clear(self.gc);
        for observer in &taken {
            observer.pending.set(false);
        }
        taken
    }

    /// `observer`'s queue, taken (`takeRecords()`, the task's).
    pub fn take_intersection_entries(
        &self,
        observer: Gc<'gc, IntersectionObserver>,
    ) -> Vec<Gc<'gc, IntersectionObserverEntry>> {
        let Some(entries) = observer.entries.get(self.gc) else {
            return Vec::new();
        };
        let taken: Vec<_> = (0..entries.len())
            .filter_map(|index| entries.get(self.gc, index))
            .collect();
        entries.clear(self.gc);
        taken
    }

    pub fn intersection_entry_target(
        &self,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> NodeRef<'gc> {
        entry.target.get(self.gc).expect("an entry's target")
    }

    pub fn intersection_entry_root_bounds(
        &self,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> Option<Gc<'gc, crate::DomRect>> {
        entry.root_bounds.get(self.gc)
    }

    pub fn intersection_entry_bounding_client_rect(
        &self,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> Gc<'gc, crate::DomRect> {
        entry
            .bounding_client_rect
            .get(self.gc)
            .expect("an entry's rect")
    }

    pub fn intersection_entry_intersection_rect(
        &self,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> Gc<'gc, crate::DomRect> {
        entry
            .intersection_rect
            .get(self.gc)
            .expect("an entry's rect")
    }

    /// `entry`'s `time`, `isIntersecting`, `intersectionRatio` and
    /// `isVisible`.
    pub fn intersection_entry_values(
        &self,
        entry: Gc<'gc, IntersectionObserverEntry>,
    ) -> (f64, bool, f64, bool) {
        (
            entry.time.get(),
            entry.is_intersecting.get(),
            entry.intersection_ratio.get(),
            entry.is_visible.get(),
        )
    }
}

/// `list`'s entries `alive` says aren't let go of, now and then (as the
/// list doubles past 16): what weak lists keep of the dead stays small.
pub(crate) fn prune<'gc, T: GcObject>(
    cx: &Cx<'_, 'gc>,
    list: Gc<'gc, HeapVector<T>>,
    alive: impl Fn(Gc<'gc, T>) -> bool,
) {
    if list.len() < 16 || !list.len().is_power_of_two() {
        return;
    }
    let mut index = 0;
    while let Some(entry) = list.get(cx.gc, index) {
        if alive(entry) {
            index = index.saturating_add(1);
        } else {
            list.remove(cx.gc, index);
        }
    }
}
