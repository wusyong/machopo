//! A window's `localStorage` and `sessionStorage` (HTML's `Storage` of a
//! storage bottle map; Blink's `StorageArea` over its `CachedStorageArea`):
//! the window keeps each `Storage`, which is of an area by the host's
//! number; the DOM keeps each area's items once, for every document of its
//! key (the host's copy of the browser's area), so what one document
//! changes another of the key reads at once. What changes is queued for
//! the host, which the engine hands it before the call returns
//! (`Cx::take_storage_changes`). A `localStorage` area's changes come back
//! from the browser, this DOM's and other renderers', and fire `storage`
//! at the windows of the area but the one that made it
//! (`Cx::storage_area_changed`); a `sessionStorage` area's fire at once,
//! the tab's documents all being here.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member, Persistent, WeakMember};

use crate::{Cx, Window, sites};

#[derive(GcObject)]
pub struct Storage {
    window: Member<Window>,
    /// Its area, the host's number for it.
    area: Cell<u64>,
    /// Its number in this DOM, from 1: which `Storage` made a change, its
    /// events not fired at its own window (`StorageAreaSource`'s id).
    source: Cell<u64>,
    wrapper: JsEdge,
}

/// A `Storage` held weakly, on the DOM's list of them: what a change's
/// `storage` events go through.
#[derive(GcObject)]
pub struct StorageRef {
    storage: WeakMember<Storage>,
}

/// Which `Storage` of a window: `localStorage` or `sessionStorage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageKind {
    Local,
    Session,
}

/// What was changed in an area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageMutation {
    Set { key: String, value: String },
    Remove { key: String },
    Clear,
}

impl StorageMutation {
    /// The key it changed, none for a clear.
    fn key(&self) -> Option<&str> {
        match self {
            Self::Set { key, .. } | Self::Remove { key } => Some(key),
            Self::Clear => None,
        }
    }
}

/// A change made to an area, for the host to pass on: by the `Storage`
/// numbered `source`, of the document at `url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageChange {
    pub area: u64,
    pub mutation: StorageMutation,
    pub source: u64,
    pub url: String,
}

/// A `storage` event to fire (HTML's "broadcast"): its key (none for a
/// clear), the values before and after, the URL of the document that
/// made the change, and which of the window's `Storage`s it is of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageEventInfo {
    pub key: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub url: String,
    pub kind: StorageKind,
}

/// A `StorageEvent`'s data: its key, values and URL, and its
/// `storageArea`.
#[derive(GcObject)]
pub struct StorageEventData {
    values: LocalExternal<StorageEventValues>,
    storage_area: Member<Storage>,
}

/// A `StorageEvent`'s `key`, `oldValue`, `newValue` (each none for null)
/// and `url`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageEventValues {
    pub key: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub url: String,
}

/// An area's items, ordered by key (Chromium's `StorageAreaMap`), and,
/// for a `localStorage` area, the keys of the changes made here the
/// browser hasn't told back yet, in order (none for a clear; Blink's
/// pending mutations): another renderer's change of such a key, which the
/// browser made before this one's, is not taken.
#[derive(Default)]
struct Area {
    items: BTreeMap<String, String>,
    kind: Option<StorageKind>,
    pending: VecDeque<Option<String>>,
}

/// The areas by number, the changes the host has yet to take, the
/// windows whose `storage` listeners want their areas opened, the last
/// `Storage`'s number, and every `Storage`, weakly.
#[derive(Default)]
pub(crate) struct StorageAreas {
    areas: RefCell<HashMap<u64, Area>>,
    changes: Cell<Vec<StorageChange>>,
    listened: Cell<Vec<Persistent<Window>>>,
    sources: Cell<u64>,
    storages: Cell<Option<Persistent<HeapVector<StorageRef>>>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// `window`'s `localStorage` or `sessionStorage`, once it has one.
    pub fn window_storage(
        &self,
        window: Gc<'gc, Window>,
        kind: StorageKind,
    ) -> Option<Gc<'gc, Storage>> {
        match kind {
            StorageKind::Local => window.local_storage.get(self.gc),
            StorageKind::Session => window.session_storage.get(self.gc),
        }
    }

    /// `window`'s `Storage` of `kind` from now on: of the host's area
    /// `area`, with `items` if the DOM didn't have them (none: another
    /// document's area, which the DOM has).
    pub fn set_window_storage(
        &self,
        window: Gc<'gc, Window>,
        kind: StorageKind,
        area: u64,
        items: Option<Vec<(String, String)>>,
    ) -> Gc<'gc, Storage> {
        {
            let mut areas = self.changes.storage.areas.borrow_mut();
            let entry = areas.entry(area).or_default();
            entry.kind = Some(kind);
            if let Some(items) = items {
                entry.items = items.into_iter().collect();
                entry.pending.clear();
            }
        }
        let sources = &self.changes.storage.sources;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "two a window at most: a u64 lasts centuries"
        )]
        sources.set(sources.get() + 1);
        let storage = self.gc.alloc_in(
            &sites::OTHER,
            StorageInit {
                window: Some(window),
                area: Cell::new(area),
                source: Cell::new(sources.get()),
                wrapper: (),
            },
        );
        match kind {
            StorageKind::Local => window.local_storage.set(self.gc, Some(storage)),
            StorageKind::Session => window.session_storage.set(self.gc, Some(storage)),
        }
        let list = self.storages();
        crate::intersection::prune(self, list, |entry: Gc<'gc, StorageRef>| {
            entry.storage.get(self.gc).is_some()
        });
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            StorageRefInit {
                storage: Some(storage),
            },
        );
        list.push(self.gc, entry);
        storage
    }

    /// The DOM's list of its `Storage`s.
    fn storages(&self) -> Gc<'gc, HeapVector<StorageRef>> {
        let cell = &self.changes.storage.storages;
        let root = cell
            .take()
            .unwrap_or_else(|| Persistent::new(self.gc, HeapVector::new(self.gc, 4)));
        let list = root.get(self.gc);
        cell.set(Some(root));
        list
    }

    pub fn storage_window(&self, storage: Gc<'gc, Storage>) -> Gc<'gc, Window> {
        storage.window.get(self.gc).expect("a storage's window")
    }

    fn with_area<R>(&self, area: u64, f: impl FnOnce(&mut Area) -> R) -> R {
        let mut areas = self.changes.storage.areas.borrow_mut();
        f(areas.entry(area).or_default())
    }

    /// How many items its area has.
    pub fn storage_length(&self, storage: Gc<'gc, Storage>) -> usize {
        self.with_area(storage.area.get(), |area| area.items.len())
    }

    /// The key of the item at `index`, in the order of the keys.
    pub fn storage_key(&self, storage: Gc<'gc, Storage>, index: usize) -> Option<String> {
        self.with_area(storage.area.get(), |area| {
            area.items.keys().nth(index).cloned()
        })
    }

    /// Every key, in order.
    pub fn storage_keys(&self, storage: Gc<'gc, Storage>) -> Vec<String> {
        self.with_area(storage.area.get(), |area| {
            area.items.keys().cloned().collect()
        })
    }

    pub fn storage_get(&self, storage: Gc<'gc, Storage>, key: &str) -> Option<String> {
        self.with_area(storage.area.get(), |area| area.items.get(key).cloned())
    }

    /// `key`'s value set to `value`: nothing changes if it was that.
    pub fn storage_set(&self, storage: Gc<'gc, Storage>, key: &str, value: &str) {
        let old = self.with_area(storage.area.get(), |area| {
            if area.items.get(key).is_some_and(|old| old == value) {
                return None;
            }
            Some(area.items.insert(key.to_owned(), value.to_owned()))
        });
        if let Some(old) = old {
            self.storage_changed_here(
                storage,
                StorageMutation::Set {
                    key: key.to_owned(),
                    value: value.to_owned(),
                },
                old,
            );
        }
    }

    /// `key`'s item removed: nothing changes if there was none.
    pub fn storage_remove(&self, storage: Gc<'gc, Storage>, key: &str) {
        let old = self.with_area(storage.area.get(), |area| area.items.remove(key));
        if old.is_some() {
            self.storage_changed_here(
                storage,
                StorageMutation::Remove {
                    key: key.to_owned(),
                },
                old,
            );
        }
    }

    /// Every item removed: nothing changes if there was none.
    pub fn storage_clear(&self, storage: Gc<'gc, Storage>) {
        let had = self.with_area(storage.area.get(), |area| {
            let had = !area.items.is_empty();
            area.items.clear();
            had
        });
        if had {
            self.storage_changed_here(storage, StorageMutation::Clear, None);
        }
    }

    /// Script changed `storage`'s area (`old` the value before): the host
    /// told; a `localStorage` area's change waits to be told back, a
    /// `sessionStorage` one's fires at once.
    fn storage_changed_here(
        &self,
        storage: Gc<'gc, Storage>,
        mutation: StorageMutation,
        old: Option<String>,
    ) {
        let area = storage.area.get();
        let source = storage.source.get();
        let url = self
            .document_url(self.window_document(self.storage_window(storage)))
            .map(|url| url.to_str().to_string())
            .unwrap_or_default();
        let kind = self.with_area(area, |entry| {
            if entry.kind == Some(StorageKind::Local) {
                entry.pending.push_back(mutation.key().map(str::to_owned));
            }
            entry.kind
        });
        if kind == Some(StorageKind::Session) {
            let event = StorageEventInfo {
                key: mutation.key().map(str::to_owned),
                old_value: old,
                new_value: match &mutation {
                    StorageMutation::Set { value, .. } => Some(value.clone()),
                    _ => None,
                },
                url: url.clone(),
                kind: StorageKind::Session,
            };
            self.queue_storage_events(area, Some(source), &event);
        }
        let state = &self.changes.storage.changes;
        let mut changes = state.take();
        changes.push(StorageChange {
            area,
            mutation,
            source,
            url,
        });
        state.set(changes);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | crate::Microtasks::STORAGE);
    }

    /// The browser's news of a change to the `localStorage` area `area`
    /// (`StorageAreaObserver`): its value before, whether it changed
    /// anything, the URL of the document that made it, and `source` if a
    /// `Storage` of this DOM did (its change made here already: told
    /// back). Another's is taken into the DOM's copy unless a change made
    /// here and not told back yet comes after it for the key (Blink's
    /// `MaybeApplyNonLocalMutationForKey`). A `storage` event is queued at
    /// each window of the area but the source's.
    pub fn storage_area_changed(
        &self,
        area: u64,
        mutation: StorageMutation,
        old_value: Option<String>,
        changed: bool,
        url: String,
        source: Option<u64>,
    ) {
        let known = self.with_area(area, |entry| {
            if entry.kind != Some(StorageKind::Local) {
                return false;
            }
            if source.is_some() {
                entry.pending.pop_front();
                return true;
            }
            let pending = |key: &str| {
                entry
                    .pending
                    .iter()
                    .any(|pending| pending.as_deref().is_none_or(|pending| pending == key))
            };
            match &mutation {
                StorageMutation::Set { key, value } => {
                    if !pending(key) {
                        entry.items.insert(key.clone(), value.clone());
                    }
                }
                StorageMutation::Remove { key } => {
                    if !pending(key) {
                        entry.items.remove(key);
                    }
                }
                StorageMutation::Clear => {
                    let kept: Vec<String> = entry
                        .items
                        .keys()
                        .filter(|key| pending(key))
                        .cloned()
                        .collect();
                    entry.items.retain(|key, _| kept.contains(key));
                }
            }
            true
        });
        if !known || !changed {
            return;
        }
        let event = StorageEventInfo {
            key: mutation.key().map(str::to_owned),
            old_value,
            new_value: match &mutation {
                StorageMutation::Set { value, .. } => Some(value.clone()),
                _ => None,
            },
            url,
            kind: StorageKind::Local,
        };
        self.queue_storage_events(area, source, &event);
    }

    /// A `storage` event of `event` queued at each window whose `Storage`
    /// of its kind is of `area`, but `source`'s (HTML's "queue a global
    /// task on the DOM manipulation task source").
    fn queue_storage_events(&self, area: u64, source: Option<u64>, event: &StorageEventInfo) {
        let list = self.storages();
        let storages: Vec<Gc<'gc, Storage>> = (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .filter_map(|entry| entry.storage.get(self.gc))
            .collect();
        for storage in storages {
            if storage.area.get() != area || Some(storage.source.get()) == source {
                continue;
            }
            let window = self.storage_window(storage);
            // One a window no longer has (its DOM's areas opened again).
            if self
                .window_storage(window, event.kind)
                .is_none_or(|own| !Gc::ptr_eq(own, storage))
            {
                continue;
            }
            let cell = window.into_ref().storage_events.get();
            let mut queued = cell.take();
            queued.push(event.clone());
            cell.set(queued);
            let document = self.window_document(window);
            self.push_element_task(
                window,
                document,
                crate::names::event::STORAGE,
                crate::window::STORAGE_EVENT,
                0,
            );
        }
    }

    /// Makes `event` a `StorageEvent` of `values` and `storage_area`
    /// (again, for `initStorageEvent`).
    pub fn set_storage_event_data(
        &self,
        event: Gc<'gc, crate::Event>,
        values: StorageEventValues,
        storage_area: Option<Gc<'gc, Storage>>,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            StorageEventDataInit {
                values: LocalExternal::new(values),
                storage_area,
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s values and `storageArea`, if it is a `StorageEvent`.
    pub fn storage_event_data(
        &self,
        event: Gc<'gc, crate::Event>,
    ) -> Option<(&'gc StorageEventValues, Option<Gc<'gc, Storage>>)> {
        let data = event.data.get(self.gc)?.downcast::<StorageEventData>()?;
        let storage_area = data.storage_area.get(self.gc);
        Some((data.into_ref().values.get(), storage_area))
    }

    /// The first queued `storage` event of `window` (its task runs).
    pub(crate) fn take_storage_event(&self, window: Gc<'gc, Window>) -> Option<StorageEventInfo> {
        let cell = window.into_ref().storage_events.get();
        let mut queued = cell.take();
        let first = (!queued.is_empty()).then(|| queued.remove(0));
        cell.set(queued);
        first
    }

    /// A `storage` listener was added to `window` (an event handler too):
    /// its areas are to be opened, for it to hear of their changes
    /// (Blink's `DOMWindowStorageController::DidAddEventListener`), by the
    /// engine before the call returns (`take_storage_listened`).
    pub(crate) fn storage_listened(&self, window: Gc<'gc, Window>) {
        let state = &self.changes.storage.listened;
        let mut listened = state.take();
        listened.push(Persistent::new(self.gc, window));
        state.set(listened);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | crate::Microtasks::STORAGE);
    }

    /// The windows given `storage` listeners since the last time, taken.
    pub fn take_storage_listened(&self) -> Vec<Persistent<Window>> {
        self.changes.storage.listened.take()
    }

    /// The changes made since the last time, in order, taken.
    pub fn take_storage_changes(&self) -> Vec<StorageChange> {
        self.changes.storage.changes.take()
    }

    pub(crate) fn storage_wrapper_edge(&self, storage: Gc<'gc, Storage>) -> &'gc JsEdge {
        &storage.into_ref().wrapper
    }
}
