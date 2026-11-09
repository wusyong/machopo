//! `AbortController` and `AbortSignal` (DOM's "Aborting ongoing
//! activities", Blink's `core/dom/abort_signal.h`). An abort signal is an
//! event target of its own (`EventTargetImpl`'s `abort`) with its state: its
//! realm's window, whether it aborted and its reason (the JS engine's value;
//! a dependent signal aborted by a source has its source's, `reason_from`),
//! its abort algorithms (`addEventListener`'s `signal`: a listener to
//! remove), and `AbortSignal.any()`'s links: a dependent signal holds its
//! sources strongly; a source holds its dependents weakly, and strongly only
//! those that must not go (DOM's garbage collection rule: one not aborted,
//! with `abort` listeners or abort algorithms, whose sources can still abort
//! it), which `update_kept` keeps right (Blink's `dependent_signals_` of
//! `WeakMember`s and its `AbortSignalCompositionManager`). A controller
//! holds its signal. Firing `abort` and making a reason is `dom_api`'s.

use std::cell::Cell;

use elise::{Erased, Gc, GcObject, HeapVector, JsEdge, Member, WeakMember};

use crate::{Cx, EventTargetImpl, Listener, TargetRef, Window, names, sites};

#[derive(GcObject)]
pub struct AbortController {
    signal: Member<EventTargetImpl>,
    wrapper: JsEdge,
}

/// An abort signal's state.
#[derive(GcObject)]
pub struct AbortSignalData {
    /// The window of the realm it was made in.
    window: Member<Window>,
    aborted: Cell<bool>,
    /// DOM's "dependent": made by `AbortSignal.any()`, aborted only through
    /// its sources.
    dependent: Cell<bool>,
    /// Its abort reason, once aborted: the engine's value, or its source's
    /// (`reason_from`) for a dependent signal a source aborted.
    reason: JsEdge,
    reason_from: Member<EventTargetImpl>,
    algorithms: Member<HeapVector<AbortAlgorithm>>,
    /// A dependent signal's sources, strongly, in the order given.
    sources: Member<HeapVector<EventTargetImpl>>,
    /// A source's dependent signals, weakly, in the order made; and those
    /// of them that must not go (`update_kept`), strongly.
    dependents: Member<HeapVector<DependentRef>>,
    kept: Member<HeapVector<EventTargetImpl>>,
}

/// A source's weak reference to one of its dependent signals.
#[derive(GcObject)]
pub struct DependentRef {
    signal: WeakMember<EventTargetImpl>,
}

/// An abort algorithm (Blink's `AbortSignal::Algorithm`):
/// `addEventListener`'s, removing a listener from its target; or a
/// `fetch()`'s (`fetch`), aborting it.
#[derive(GcObject)]
pub struct AbortAlgorithm {
    target: Member<Erased>,
    listener: Member<Listener>,
    fetch: Member<crate::FetchRecord>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new controller, with a new signal of `window`'s realm.
    pub fn create_abort_controller(&self, window: Gc<'gc, Window>) -> Gc<'gc, AbortController> {
        let signal = self.create_abort_signal(window);
        self.gc.alloc_in(
            &sites::OTHER,
            AbortControllerInit {
                signal: Some(signal),
                wrapper: (),
            },
        )
    }

    pub fn abort_controller_signal(
        &self,
        controller: Gc<'gc, AbortController>,
    ) -> Gc<'gc, EventTargetImpl> {
        controller
            .signal
            .get(self.gc)
            .expect("a controller's signal")
    }

    /// A new abort signal of `window`'s realm, not aborted.
    pub fn create_abort_signal(&self, window: Gc<'gc, Window>) -> Gc<'gc, EventTargetImpl> {
        let target = self.create_event_target();
        let data = self.gc.alloc_in(
            &sites::OTHER,
            AbortSignalDataInit {
                window: Some(window),
                aborted: Cell::new(false),
                dependent: Cell::new(false),
                reason: (),
                reason_from: None,
                algorithms: None,
                sources: None,
                dependents: None,
                kept: None,
            },
        );
        target.abort.set(self.gc, Some(data));
        target
    }

    pub fn is_abort_signal(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        target.abort.get(self.gc).is_some()
    }

    fn signal_data(&self, signal: Gc<'gc, EventTargetImpl>) -> Gc<'gc, AbortSignalData> {
        signal.abort.get(self.gc).expect("an abort signal")
    }

    /// The window of the realm `target` was made in, if it is a signal.
    pub fn abort_signal_window(&self, target: Gc<'gc, EventTargetImpl>) -> Option<Gc<'gc, Window>> {
        target.abort.get(self.gc)?.window.get(self.gc)
    }

    pub fn abort_signal_aborted(&self, signal: Gc<'gc, EventTargetImpl>) -> bool {
        self.signal_data(signal).aborted.get()
    }

    /// The edge `signal`'s own abort reason is in, which only the engine
    /// reads and writes (set before `signal_abort`).
    pub fn abort_reason_edge(&self, signal: Gc<'gc, EventTargetImpl>) -> &'gc JsEdge {
        &self.signal_data(signal).into_ref().reason
    }

    /// The edge `signal`'s abort reason is in, once it aborted: its own, or
    /// that of the source that aborted it.
    pub fn abort_reason(&self, signal: Gc<'gc, EventTargetImpl>) -> Option<&'gc JsEdge> {
        let mut data = self.signal_data(signal);
        if !data.aborted.get() {
            return None;
        }
        while let Some(source) = data.reason_from.get(self.gc) {
            data = self.signal_data(source);
        }
        Some(&data.into_ref().reason)
    }

    /// DOM's "create a dependent abort signal" from `sources` (each an
    /// abort signal), of `window`'s realm: aborted already, with its
    /// reason, if one of them is; else linked to each source that is no
    /// dependent and the sources of each that is.
    pub fn create_dependent_abort_signal(
        &self,
        window: Gc<'gc, Window>,
        sources: &[Gc<'gc, EventTargetImpl>],
    ) -> Gc<'gc, EventTargetImpl> {
        let signal = self.create_abort_signal(window);
        let data = self.signal_data(signal);
        data.dependent.set(true);
        if let Some(&aborted) = sources
            .iter()
            .find(|&&source| self.abort_signal_aborted(source))
        {
            data.aborted.set(true);
            data.reason_from.set(self.gc, Some(aborted));
            return signal;
        }
        let mut linked: Vec<Gc<'gc, EventTargetImpl>> = Vec::new();
        for &source in sources {
            let source_data = self.signal_data(source);
            let of = if source_data.dependent.get() {
                self.signal_sources(source)
            } else {
                vec![source]
            };
            for source in of {
                if linked.iter().any(|&other| Gc::ptr_eq(other, source)) {
                    continue;
                }
                linked.push(source);
                let list = vector(self, &data.into_ref().sources);
                list.push(self.gc, source);
                let dependent = self.gc.alloc_in(
                    &sites::OTHER,
                    DependentRefInit {
                        signal: Some(signal),
                    },
                );
                let dependents = vector(self, &self.signal_data(source).into_ref().dependents);
                // The ones gone, out first: a source `any()` is called
                // on again and again doesn't keep their references.
                let mut index = 0;
                while index < dependents.len() {
                    let alive = dependents
                        .get(self.gc, index)
                        .is_some_and(|other| other.signal.get(self.gc).is_some());
                    if alive {
                        index = index.saturating_add(1);
                    } else {
                        dependents.remove(self.gc, index);
                    }
                }
                dependents.push(self.gc, dependent);
            }
        }
        signal
    }

    /// A dependent signal's sources, in order (none for another signal).
    pub fn signal_sources(
        &self,
        signal: Gc<'gc, EventTargetImpl>,
    ) -> Vec<Gc<'gc, EventTargetImpl>> {
        self.signal_data(signal)
            .sources
            .get(self.gc)
            .map_or_else(Vec::new, |sources| {
                (0..sources.len())
                    .filter_map(|index| sources.get(self.gc, index))
                    .collect()
            })
    }

    /// DOM's "signal abort" of `signal` (not aborted; its reason set in
    /// `abort_reason_edge` first), the state's part: it and each of its
    /// dependent signals still alive and not aborted are aborted, the
    /// dependents with its reason; those, in order, it first, whose abort
    /// steps (`take_abort_algorithms`, `abort`) are to run.
    pub fn signal_abort(&self, signal: Gc<'gc, EventTargetImpl>) -> Vec<Gc<'gc, EventTargetImpl>> {
        let data = self.signal_data(signal);
        debug_assert!(!data.aborted.get(), "a signal aborts once");
        data.aborted.set(true);
        let mut aborted = vec![signal];
        if let Some(dependents) = data.dependents.get(self.gc) {
            for index in 0..dependents.len() {
                let Some(dependent) = dependents
                    .get(self.gc, index)
                    .and_then(|dependent| dependent.signal.get(self.gc))
                else {
                    continue;
                };
                let dependent_data = self.signal_data(dependent);
                if dependent_data.aborted.get() {
                    continue;
                }
                dependent_data.aborted.set(true);
                dependent_data.reason_from.set(self.gc, Some(signal));
                aborted.push(dependent);
            }
        }
        // Aborted, nothing is kept for it any more, and it keeps none.
        data.dependents.set(self.gc, None);
        data.kept.set(self.gc, None);
        for &dependent in &aborted[1..] {
            self.update_kept(dependent);
        }
        aborted
    }

    /// An abort algorithm of `signal`'s (not aborted): `listener` of
    /// `target` removed as it aborts (`addEventListener`'s `signal`).
    pub fn add_listener_abort_algorithm(
        &self,
        signal: Gc<'gc, EventTargetImpl>,
        target: TargetRef<'gc>,
        listener: Gc<'gc, Listener>,
    ) {
        let algorithm = self.gc.alloc_in(
            &sites::OTHER,
            AbortAlgorithmInit {
                target: Some(target.erase()),
                listener: Some(listener),
                fetch: None,
            },
        );
        vector(self, &self.signal_data(signal).into_ref().algorithms).push(self.gc, algorithm);
        self.update_kept(signal);
    }

    /// A `fetch()`'s abort algorithm of `signal`'s (not aborted).
    pub(crate) fn add_fetch_abort_algorithm(
        &self,
        signal: Gc<'gc, EventTargetImpl>,
        fetch: Gc<'gc, crate::FetchRecord>,
    ) {
        let algorithm = self.gc.alloc_in(
            &sites::OTHER,
            AbortAlgorithmInit {
                target: None,
                listener: None,
                fetch: Some(fetch),
            },
        );
        vector(self, &self.signal_data(signal).into_ref().algorithms).push(self.gc, algorithm);
        self.update_kept(signal);
    }

    /// `fetch`'s abort algorithm of `signal`'s taken out (its response
    /// came).
    pub(crate) fn remove_fetch_abort_algorithm(
        &self,
        signal: Gc<'gc, EventTargetImpl>,
        fetch: Gc<'gc, crate::FetchRecord>,
    ) {
        let Some(algorithms) = self.signal_data(signal).algorithms.get(self.gc) else {
            return;
        };
        if let Some(index) = (0..algorithms.len()).find(|&index| {
            algorithms
                .get(self.gc, index)
                .and_then(|algorithm| algorithm.fetch.get(self.gc))
                .is_some_and(|other| Gc::ptr_eq(other, fetch))
        }) {
            algorithms.remove(self.gc, index);
        }
        self.update_kept(signal);
    }

    /// `signal`'s abort algorithms run, in the order added, and emptied:
    /// each listener removed from its target; the `fetch()`s they abort,
    /// in that order, for `dom_api` to abort.
    pub fn run_abort_algorithms(
        &self,
        signal: Gc<'gc, EventTargetImpl>,
    ) -> Vec<Gc<'gc, crate::FetchRecord>> {
        let data = self.signal_data(signal);
        let Some(algorithms) = data.algorithms.get(self.gc) else {
            return Vec::new();
        };
        data.algorithms.set(self.gc, None);
        let mut fetches = Vec::new();
        for algorithm in (0..algorithms.len()).filter_map(|index| algorithms.get(self.gc, index)) {
            if let Some(fetch) = algorithm.fetch.get(self.gc) {
                fetches.push(fetch);
                continue;
            }
            let (Some(target), Some(listener)) = (
                algorithm.target.get(self.gc),
                algorithm.listener.get(self.gc),
            ) else {
                continue;
            };
            self.remove_listener(TargetRef::from_erased(target), listener);
        }
        self.update_kept(signal);
        fetches
    }

    /// Whether `signal` must not go (DOM's garbage collection rule for a
    /// dependent signal): not aborted, with `abort` listeners or abort
    /// algorithms.
    fn must_be_kept(&self, signal: Gc<'gc, EventTargetImpl>) -> bool {
        let data = self.signal_data(signal);
        if data.aborted.get() || !data.dependent.get() {
            return false;
        }
        let algorithms = data
            .algorithms
            .get(self.gc)
            .is_some_and(|list| !list.is_empty());
        algorithms
            || self
                .listeners(TargetRef::EventTarget(signal))
                .is_some_and(|listeners| {
                    (0..listeners.len())
                        .filter_map(|index| listeners.get(self.gc, index))
                        .any(|listener| self.listener_type(listener) == names::event::ABORT)
                })
    }

    /// `signal`'s sources hold it strongly if it must not go, else weakly
    /// only: called as its listeners of `abort`, its algorithms or its
    /// state change. The one place that decides it.
    pub(crate) fn update_kept(&self, signal: Gc<'gc, EventTargetImpl>) {
        if !self.signal_data(signal).dependent.get() {
            return;
        }
        let keep = self.must_be_kept(signal);
        for source in self.signal_sources(signal) {
            let source_data = self.signal_data(source);
            if source_data.aborted.get() {
                continue;
            }
            let kept = vector(self, &source_data.into_ref().kept);
            let index = (0..kept.len()).find(|&index| {
                kept.get(self.gc, index)
                    .is_some_and(|other| Gc::ptr_eq(other, signal))
            });
            match (keep, index) {
                (true, None) => kept.push(self.gc, signal),
                (false, Some(index)) => {
                    kept.remove(self.gc, index);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn abort_controller_wrapper_edge(
        &self,
        controller: Gc<'gc, AbortController>,
    ) -> &'gc JsEdge {
        &controller.into_ref().wrapper
    }
}

/// The vector `member` holds, made the first time.
fn vector<'gc, T: elise::HeapType>(
    cx: &Cx<'_, 'gc>,
    member: &Member<HeapVector<T>>,
) -> Gc<'gc, HeapVector<T>> {
    match member.get(cx.gc) {
        Some(list) => list,
        None => {
            let list = HeapVector::new(cx.gc, 1);
            member.set(cx.gc, Some(list));
            list
        }
    }
}
