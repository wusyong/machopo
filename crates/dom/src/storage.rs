//! A window's `localStorage` (HTML's `Storage` of the local storage
//! bottle map; Blink's `StorageArea` over its `CachedStorageArea`): the
//! window keeps its `Storage`, which is of an area by the host's number;
//! the DOM keeps each area's items once, for every document of its key
//! (the host's copy of the browser's area), so what one document changes
//! another of the key reads at once. What changes is queued for the host,
//! which the engine hands it before the call returns
//! (`Cx::take_storage_changes`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use elise::{Gc, GcObject, JsEdge, Member};

use crate::{Cx, Window, sites};

#[derive(GcObject)]
pub struct Storage {
    window: Member<Window>,
    /// Its area, the host's number for it.
    area: Cell<u64>,
    wrapper: JsEdge,
}

/// A change made to an area, for the host to pass on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageChange {
    Set {
        area: u64,
        key: String,
        value: String,
    },
    Remove {
        area: u64,
        key: String,
    },
    Clear {
        area: u64,
    },
}

/// The areas' items, by number, ordered by key (Chromium's
/// `StorageAreaMap`), and the changes the host has yet to take.
#[derive(Default)]
pub(crate) struct StorageAreas {
    areas: RefCell<HashMap<u64, BTreeMap<String, String>>>,
    changes: Cell<Vec<StorageChange>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// `window`'s `localStorage`, once it has one.
    pub fn window_local_storage(&self, window: Gc<'gc, Window>) -> Option<Gc<'gc, Storage>> {
        window.local_storage.get(self.gc)
    }

    /// `window`'s `localStorage` from now on: of the host's area `area`,
    /// with `items` if the DOM didn't have them (none: another document's
    /// area, which the DOM has).
    pub fn set_window_local_storage(
        &self,
        window: Gc<'gc, Window>,
        area: u64,
        items: Option<Vec<(String, String)>>,
    ) -> Gc<'gc, Storage> {
        let mut areas = self.changes.storage.areas.borrow_mut();
        match items {
            Some(items) => {
                areas.insert(area, items.into_iter().collect());
            }
            None => {
                areas.entry(area).or_default();
            }
        }
        let storage = self.gc.alloc_in(
            &sites::OTHER,
            StorageInit {
                window: Some(window),
                area: Cell::new(area),
                wrapper: (),
            },
        );
        window.local_storage.set(self.gc, Some(storage));
        storage
    }

    pub fn storage_window(&self, storage: Gc<'gc, Storage>) -> Gc<'gc, Window> {
        storage.window.get(self.gc).expect("a storage's window")
    }

    fn with_items<R>(
        &self,
        storage: Gc<'gc, Storage>,
        f: impl FnOnce(&mut BTreeMap<String, String>) -> R,
    ) -> R {
        let mut areas = self.changes.storage.areas.borrow_mut();
        f(areas.entry(storage.area.get()).or_default())
    }

    /// How many items its area has.
    pub fn storage_length(&self, storage: Gc<'gc, Storage>) -> usize {
        self.with_items(storage, |items| items.len())
    }

    /// The key of the item at `index`, in the order of the keys.
    pub fn storage_key(&self, storage: Gc<'gc, Storage>, index: usize) -> Option<String> {
        self.with_items(storage, |items| items.keys().nth(index).cloned())
    }

    /// Every key, in order.
    pub fn storage_keys(&self, storage: Gc<'gc, Storage>) -> Vec<String> {
        self.with_items(storage, |items| items.keys().cloned().collect())
    }

    pub fn storage_get(&self, storage: Gc<'gc, Storage>, key: &str) -> Option<String> {
        self.with_items(storage, |items| items.get(key).cloned())
    }

    /// `key`'s value set to `value`: nothing changes if it was that.
    pub fn storage_set(&self, storage: Gc<'gc, Storage>, key: &str, value: &str) {
        let area = storage.area.get();
        let changed = self.with_items(storage, |items| {
            if items.get(key).is_some_and(|old| old == value) {
                return false;
            }
            items.insert(key.to_owned(), value.to_owned());
            true
        });
        if changed {
            self.queue_storage_change(StorageChange::Set {
                area,
                key: key.to_owned(),
                value: value.to_owned(),
            });
        }
    }

    /// `key`'s item removed: nothing changes if there was none.
    pub fn storage_remove(&self, storage: Gc<'gc, Storage>, key: &str) {
        let area = storage.area.get();
        if self.with_items(storage, |items| items.remove(key).is_some()) {
            self.queue_storage_change(StorageChange::Remove {
                area,
                key: key.to_owned(),
            });
        }
    }

    /// Every item removed: nothing changes if there was none.
    pub fn storage_clear(&self, storage: Gc<'gc, Storage>) {
        let area = storage.area.get();
        let changed = self.with_items(storage, |items| {
            let had = !items.is_empty();
            items.clear();
            had
        });
        if changed {
            self.queue_storage_change(StorageChange::Clear { area });
        }
    }

    fn queue_storage_change(&self, change: StorageChange) {
        let state = &self.changes.storage.changes;
        let mut changes = state.take();
        changes.push(change);
        state.set(changes);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | crate::Microtasks::STORAGE);
    }

    /// The changes made since the last time, in order, taken.
    pub fn take_storage_changes(&self) -> Vec<StorageChange> {
        self.changes.storage.changes.take()
    }

    pub(crate) fn storage_wrapper_edge(&self, storage: Gc<'gc, Storage>) -> &'gc JsEdge {
        &storage.into_ref().wrapper
    }
}
