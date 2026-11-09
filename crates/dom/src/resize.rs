//! Resize observers (Resize Observer's `ResizeObserver`; Blink's
//! `core/resize_observer/`): the GC types, and who keeps whom. The sizes
//! are layout's, read by the engine (`dom_api`'s gather and
//! broadcast, in the rendering update).
//!
//! - **Who keeps whom**: an element keeps its observations (its
//!   `ElementData`'s list), an observation its observer; an observer keeps
//!   its observations only weakly, so it doesn't keep the elements it
//!   observes alive (Blink's `ResizeObserverData` on the element).
//! - **A window's observers** (Blink's `ResizeObserverController`): every
//!   observer made for it, weakly, in the order they were made.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member, WeakMember};

use crate::{Cx, Node, NodeRef, Window, sites};

/// Which of its target's boxes an observation watches (Resize Observer's
/// `ResizeObserverBoxOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeObserverBox {
    ContentBox,
    BorderBox,
    DevicePixelContentBox,
}

impl ResizeObserverBox {
    /// Its number, as an observation keeps it (a GC object's fields are
    /// plain data).
    fn code(self) -> u8 {
        match self {
            Self::ContentBox => 0,
            Self::BorderBox => 1,
            Self::DevicePixelContentBox => 2,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::BorderBox,
            2 => Self::DevicePixelContentBox,
            _ => Self::ContentBox,
        }
    }
}

/// A box's size, CSS px (device px for the device-pixel content box);
/// machopo has no writing modes: inline is the width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxSize {
    pub inline: f64,
    pub block: f64,
}

/// `new ResizeObserver(callback)`.
#[derive(GcObject)]
pub struct ResizeObserver {
    callback: JsEdge,
    /// Its observations, weakly: the elements keep them.
    observations: Member<HeapVector<ResizeObservationRef>>,
    /// Whether it is a document's observer of its lazy `<img>`s with
    /// `sizes="auto"` (no script's: the engine selects their sources
    /// again in its stead), delivered before the others (Blink's
    /// `DeliveryTime::kBeforeOthers`).
    auto_sizes: Cell<bool>,
    wrapper: JsEdge,
}

/// An observation held weakly, on its observer's list.
#[derive(GcObject)]
pub struct ResizeObservationRef {
    observation: WeakMember<ResizeObservation>,
}

/// An observer held weakly, on its window's list.
#[derive(GcObject)]
pub struct ResizeObserverRef {
    observer: WeakMember<ResizeObserver>,
}

/// An observation (the spec's `ResizeObservation`): on its target's list,
/// which keeps it.
#[derive(GcObject)]
pub struct ResizeObservation {
    observer: Member<ResizeObserver>,
    /// The element it is on, which keeps it: the edge back only makes a
    /// cycle the GC collects with the element.
    target: Member<Node>,
    /// `ResizeObserverBox::code`'s.
    observed_box: Cell<u8>,
    /// The size last reported (the spec's `lastReportedSizes`); NaN before
    /// the first, so that the first gather always finds it changed (the
    /// spec's -1 × -1, Blink's indefinite size).
    reported_inline: Cell<f64>,
    reported_block: Cell<f64>,
}

/// A `ResizeObserverEntry`: fixed when it is made, its lists of one size
/// each the same objects every time.
#[derive(GcObject)]
pub struct ResizeObserverEntry {
    target: Member<Node>,
    content_rect: Member<crate::DomRect>,
    border_box_size: Member<ResizeObserverSize>,
    content_box_size: Member<ResizeObserverSize>,
    device_pixel_content_box_size: Member<ResizeObserverSize>,
    wrapper: JsEdge,
}

/// A `ResizeObserverSize`.
#[derive(GcObject)]
pub struct ResizeObserverSize {
    inline: Cell<f64>,
    block: Cell<f64>,
    wrapper: JsEdge,
}

/// An observation as the rendering update reads it.
#[derive(Clone, Copy, Debug)]
pub struct ResizeObservationState<'gc> {
    pub target: NodeRef<'gc>,
    pub observed_box: ResizeObserverBox,
    /// What it last reported; none before the first.
    pub last_reported: Option<BoxSize>,
}

/// What a new entry is made with: the content rect (`x`, `y`, width,
/// height, CSS px) and the three sizes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResizeObserverEntryValues {
    pub content_rect: [f64; 4],
    pub border_box: BoxSize,
    pub content_box: BoxSize,
    pub device_pixel_content_box: BoxSize,
}

impl<'gc> Cx<'_, 'gc> {
    /// `new ResizeObserver(callback)` for `window`: one of its observers.
    /// Its callback is the engine's to set (`resize_callback_edge`).
    pub fn create_resize_observer(&self, window: Gc<'gc, Window>) -> Gc<'gc, ResizeObserver> {
        let observer = self.gc.alloc_in(
            &sites::OTHER,
            ResizeObserverInit {
                callback: (),
                observations: None,
                auto_sizes: Cell::new(false),
                wrapper: (),
            },
        );
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            ResizeObserverRefInit {
                observer: Some(observer),
            },
        );
        let list = match window.resize_observers.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                window.resize_observers.set(self.gc, Some(list));
                list
            }
        };
        crate::intersection::prune(self, list, |entry: Gc<'gc, ResizeObserverRef>| {
            entry.observer.get(self.gc).is_some()
        });
        list.push(self.gc, entry);
        observer
    }

    /// `observer`'s reference to its callback, which only the engine reads
    /// and writes.
    pub fn resize_callback_edge(&self, observer: Gc<'gc, ResizeObserver>) -> &'gc JsEdge {
        &observer.into_ref().callback
    }

    pub(crate) fn resize_observer_wrapper_edge(
        &self,
        observer: Gc<'gc, ResizeObserver>,
    ) -> &'gc JsEdge {
        &observer.into_ref().wrapper
    }

    pub(crate) fn resize_entry_wrapper_edge(
        &self,
        entry: Gc<'gc, ResizeObserverEntry>,
    ) -> &'gc JsEdge {
        &entry.into_ref().wrapper
    }

    pub(crate) fn resize_size_wrapper_edge(
        &self,
        size: Gc<'gc, ResizeObserverSize>,
    ) -> &'gc JsEdge {
        &size.into_ref().wrapper
    }

    fn resize_changed(&self) {
        let version = &self.changes.resize_version;
        version.set(version.get().wrapping_add(1));
    }

    /// How many times targets were observed: a new observation asks for a
    /// frame, whose rendering update gathers it.
    pub fn resize_version(&self) -> u64 {
        self.changes.resize_version.get()
    }

    fn element_resize_observations(
        &self,
        target: NodeRef<'gc>,
        make: bool,
    ) -> Option<Gc<'gc, HeapVector<ResizeObservation>>> {
        if let Some(list) = self
            .existing_element_data(target)
            .and_then(|data| data.resize_observations.get(self.gc))
        {
            return Some(list);
        }
        if !make {
            return None;
        }
        let list = HeapVector::new(self.gc, 1);
        self.element_data(target)
            .resize_observations
            .set(self.gc, Some(list));
        Some(list)
    }

    /// `observe(target, options)`: an observation of `target`'s
    /// `observed_box`; nothing if there is one of that box already (Blink's
    /// `ResizeObserver::observeInternal`: what it reported stands), else in
    /// place of one of another box (Resize Observer: the old one removed, a
    /// new one added at the end).
    pub fn observe_resize(
        &self,
        observer: Gc<'gc, ResizeObserver>,
        target: NodeRef<'gc>,
        observed_box: ResizeObserverBox,
    ) {
        if self.kind(target) != crate::NodeKind::Element {
            return;
        }
        if self
            .resize_observation_of(observer, target)
            .is_some_and(|observation| observation.observed_box.get() == observed_box.code())
        {
            return;
        }
        self.unobserve_resize(observer, target);
        let observation = self.gc.alloc_in(
            &sites::OTHER,
            ResizeObservationInit {
                observer: Some(observer),
                target: Some(target),
                observed_box: Cell::new(observed_box.code()),
                reported_inline: Cell::new(f64::NAN),
                reported_block: Cell::new(f64::NAN),
            },
        );
        self.element_resize_observations(target, true)
            .expect("made")
            .push(self.gc, observation);
        let reference = self.gc.alloc_in(
            &sites::OTHER,
            ResizeObservationRefInit {
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
        crate::intersection::prune(self, list, |reference: Gc<'gc, ResizeObservationRef>| {
            reference
                .observation
                .get(self.gc)
                .is_some_and(|observation| observation.observer.get(self.gc).is_some())
        });
        list.push(self.gc, reference);
        self.resize_changed();
    }

    /// `unobserve(target)`: `observer`'s observation of it taken off.
    pub fn unobserve_resize(&self, observer: Gc<'gc, ResizeObserver>, target: NodeRef<'gc>) {
        let Some(list) = self.element_resize_observations(target, false) else {
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
            } else {
                index = index.saturating_add(1);
            }
        }
    }

    /// `disconnect()`: every observation of `observer`'s taken off.
    pub fn disconnect_resize_observer(&self, observer: Gc<'gc, ResizeObserver>) {
        for state in self.resize_observations(observer) {
            self.unobserve_resize(observer, state.target);
        }
        if let Some(list) = observer.observations.get(self.gc) {
            list.clear(self.gc);
        }
    }

    fn resize_observation_of(
        &self,
        observer: Gc<'gc, ResizeObserver>,
        target: NodeRef<'gc>,
    ) -> Option<Gc<'gc, ResizeObservation>> {
        let list = self.element_resize_observations(target, false)?;
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .find(|observation| {
                observation
                    .observer
                    .get(self.gc)
                    .is_some_and(|of| Gc::ptr_eq(of, observer))
            })
    }

    /// `observer`'s observations (those alive, on their targets still), in
    /// the order they were made.
    pub fn resize_observations(
        &self,
        observer: Gc<'gc, ResizeObserver>,
    ) -> Vec<ResizeObservationState<'gc>> {
        let Some(list) = observer.observations.get(self.gc) else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.observation.get(self.gc))
            .filter(|observation| observation.observer.get(self.gc).is_some())
            .filter_map(|observation| {
                let inline = observation.reported_inline.get();
                Some(ResizeObservationState {
                    target: observation.target.get(self.gc)?,
                    observed_box: ResizeObserverBox::from_code(observation.observed_box.get()),
                    last_reported: (!inline.is_nan()).then(|| BoxSize {
                        inline,
                        block: observation.reported_block.get(),
                    }),
                })
            })
            .collect()
    }

    /// `window`'s observers, alive, in the order they were made, its
    /// documents' own first.
    pub fn window_resize_observers(&self, window: Gc<'gc, Window>) -> Vec<Gc<'gc, ResizeObserver>> {
        let Some(list) = window.resize_observers.get(self.gc) else {
            return Vec::new();
        };
        let mut observers: Vec<_> = (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.observer.get(self.gc))
            .collect();
        observers.sort_by_key(|observer| !observer.auto_sizes.get());
        observers
    }

    /// `document`'s observer of its lazy `<img>`s with `sizes="auto"`,
    /// made at the first; none without a window.
    pub(crate) fn auto_sizes_observer(
        &self,
        document: NodeRef<'gc>,
    ) -> Option<Gc<'gc, ResizeObserver>> {
        let window = self.window_of(document)?;
        let data = self.document_data(document)?;
        if let Some(observer) = data.auto_sizes_observer.get(self.gc) {
            return Some(observer);
        }
        let observer = self.create_resize_observer(window);
        observer.auto_sizes.set(true);
        data.auto_sizes_observer.set(self.gc, Some(observer));
        Some(observer)
    }

    /// Whether `observer` is a document's observer of its lazy `<img>`s
    /// with `sizes="auto"`.
    pub fn is_auto_sizes_observer(&self, observer: Gc<'gc, ResizeObserver>) -> bool {
        observer.auto_sizes.get()
    }

    /// `element`, an `<img>` whose source size is its width
    /// (`sizes="auto"`, lazy), observed by its document's observer of
    /// those (when `observe`), else not (Blink's `HTMLImageElement::
    /// SourceSize`).
    pub fn observe_auto_sizes(&self, element: NodeRef<'gc>, observe: bool) {
        let document = self.document_of(element);
        if observe {
            if let Some(observer) = self.auto_sizes_observer(document) {
                self.observe_resize(observer, element, ResizeObserverBox::ContentBox);
            }
        } else if let Some(observer) = self
            .document_data(document)
            .and_then(|data| data.auto_sizes_observer.get(self.gc))
        {
            self.unobserve_resize(observer, element);
        }
    }

    /// `observer`'s observation of `target` reported `size`.
    pub fn set_resize_observation_reported(
        &self,
        observer: Gc<'gc, ResizeObserver>,
        target: NodeRef<'gc>,
        size: BoxSize,
    ) {
        if let Some(observation) = self.resize_observation_of(observer, target) {
            observation.reported_inline.set(size.inline);
            observation.reported_block.set(size.block);
        }
    }

    fn new_resize_size(&self, size: BoxSize) -> Gc<'gc, ResizeObserverSize> {
        self.gc.alloc_in(
            &sites::OTHER,
            ResizeObserverSizeInit {
                inline: Cell::new(size.inline),
                block: Cell::new(size.block),
                wrapper: (),
            },
        )
    }

    /// A new entry of `target`'s.
    pub fn create_resize_entry(
        &self,
        target: NodeRef<'gc>,
        values: ResizeObserverEntryValues,
    ) -> Gc<'gc, ResizeObserverEntry> {
        let content_rect = self.new_rect_of(values.content_rect, true);
        let border_box_size = self.new_resize_size(values.border_box);
        let content_box_size = self.new_resize_size(values.content_box);
        let device_pixel_content_box_size = self.new_resize_size(values.device_pixel_content_box);
        self.gc.alloc_in(
            &sites::OTHER,
            ResizeObserverEntryInit {
                target: Some(target),
                content_rect: Some(content_rect),
                border_box_size: Some(border_box_size),
                content_box_size: Some(content_box_size),
                device_pixel_content_box_size: Some(device_pixel_content_box_size),
                wrapper: (),
            },
        )
    }

    pub fn resize_entry_target(&self, entry: Gc<'gc, ResizeObserverEntry>) -> NodeRef<'gc> {
        entry.target.get(self.gc).expect("an entry's target")
    }

    pub fn resize_entry_content_rect(
        &self,
        entry: Gc<'gc, ResizeObserverEntry>,
    ) -> Gc<'gc, crate::DomRect> {
        entry.content_rect.get(self.gc).expect("an entry's rect")
    }

    /// `entry`'s size of `observed_box`'s.
    pub fn resize_entry_size(
        &self,
        entry: Gc<'gc, ResizeObserverEntry>,
        observed_box: ResizeObserverBox,
    ) -> Gc<'gc, ResizeObserverSize> {
        match observed_box {
            ResizeObserverBox::BorderBox => entry.border_box_size.get(self.gc),
            ResizeObserverBox::ContentBox => entry.content_box_size.get(self.gc),
            ResizeObserverBox::DevicePixelContentBox => {
                entry.device_pixel_content_box_size.get(self.gc)
            }
        }
        .expect("an entry's sizes")
    }

    pub fn resize_size(&self, size: Gc<'gc, ResizeObserverSize>) -> BoxSize {
        BoxSize {
            inline: size.inline.get(),
            block: size.block.get(),
        }
    }
}
