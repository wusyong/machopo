//! Web Storage's areas (Chromium's `LocalStorageImpl` and
//! `SessionStorageImpl` in the storage service, `DOMStorageContextWrapper`
//! the browser's handle on them): each storage key's items in
//! `localStorage`, kept on a thread of their own and in the profile's
//! `LocalStorage` database, and in each tab's `sessionStorage`, in memory,
//! for the renderers that opened them. A renderer gets each area once,
//! with its items, and keeps its own copy (Blink's `CachedStorageArea`):
//! what it changes it changes first, then tells; every renderer with a
//! `localStorage` area is told what changes in it, for its `storage`
//! events.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::thread::JoinHandle;

use base::SequenceNumber;
use crossbeam_channel::{Receiver, Sender};
use protocol::TabId;
use protocol::renderer::{OpenedStorageArea, RendererMsg, StorageChange, StorageKind};
use site::StorageKey;
use sql::Database;
use sql::rusqlite::params;

use crate::Profile;

/// The database's name in the profile's folder.
const DATABASE: &str = "LocalStorage";

/// The database's schema, by version.
const MIGRATIONS: &[&str] = &["CREATE TABLE items (
        storage_key TEXT NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (storage_key, key)
    ) WITHOUT ROWID;"];

/// The way to a renderer's main thread, for the answers and the changes
/// it is told of (the renderer's end of its `StorageArea` pipes).
pub type ToRenderer = Arc<dyn Fn(RendererMsg) + Send + Sync>;

/// The browser's handle on the profile's Web Storage: `localStorage`'s,
/// and each tab's `sessionStorage` (Chromium's `SessionStorageImpl`, in
/// memory alone: machopo has no session restore). Clones share the
/// thread, which ends once the last has gone, what it was told written by
/// then.
#[derive(Clone)]
pub struct DomStorageContext {
    shared: Arc<Shared>,
}

struct Shared {
    to_thread: Option<Sender<Task>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        // The thread ends once its queue is empty, its database closed:
        // the next machopo of the profile opens it.
        drop(self.to_thread.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// An area's place: a storage key's in `localStorage` (no namespace), or
/// in a tab's `sessionStorage` namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AreaKey {
    namespace: Option<TabId>,
    key: StorageKey,
}

enum Task {
    Open {
        renderer: u64,
        to_renderer: ToRenderer,
        area: AreaKey,
        request_id: u64,
    },
    Change {
        renderer: u64,
        area: u64,
        change: StorageChange,
        source: u64,
        url: String,
    },
    Forget {
        renderer: u64,
    },
    ForgetNamespace {
        namespace: TabId,
    },
}

impl DomStorageContext {
    /// `profile`'s: `localStorage` on disk in its folder, if it has one
    /// (the database opened as it is first needed), else in memory.
    pub fn new(profile: &Profile) -> std::io::Result<Self> {
        let (to_thread, tasks) = crossbeam_channel::unbounded();
        let profile = profile.clone();
        let thread = std::thread::Builder::new()
            .name("LocalStorage".into())
            .spawn(move || LocalStorage::new(profile).run(&tasks))?;
        Ok(Self {
            shared: Arc::new(Shared {
                to_thread: Some(to_thread),
                thread: Some(thread),
            }),
        })
    }

    /// Renderer `renderer`'s frame of `key` opens its area of `kind` (a
    /// `sessionStorage`'s in tab `tab`'s namespace): the renderer is told
    /// (`StorageAreaOpened`, numbered `request_id`) its area, with its
    /// items unless it has it already, or none for an opaque origin's key
    /// (no storage, HTML's `SecurityError`). It is told of the changes to
    /// a `localStorage` area from then on, through `to_renderer`.
    pub fn open(
        &self,
        renderer: u64,
        to_renderer: ToRenderer,
        kind: StorageKind,
        tab: TabId,
        key: StorageKey,
        request_id: u64,
    ) {
        let namespace = match kind {
            StorageKind::Local => None,
            StorageKind::Session => Some(tab),
        };
        self.send(Task::Open {
            renderer,
            to_renderer,
            area: AreaKey { namespace, key },
            request_id,
        });
    }

    /// Renderer `renderer` changed its area `area`, its `Storage` numbered
    /// `source` of the document at `url` did: nothing if it isn't one the
    /// renderer opened.
    pub fn change(
        &self,
        renderer: u64,
        area: u64,
        change: StorageChange,
        source: u64,
        url: String,
    ) {
        self.send(Task::Change {
            renderer,
            area,
            change,
            source,
            url,
        });
    }

    /// Renderer `renderer` has none of its areas any more (it has gone, or
    /// its DOM did): it opens them again.
    pub fn forget(&self, renderer: u64) {
        self.send(Task::Forget { renderer });
    }

    /// Tab `tab` has closed: its `sessionStorage` goes.
    pub fn forget_namespace(&self, tab: TabId) {
        self.send(Task::ForgetNamespace { namespace: tab });
    }

    fn send(&self, task: Task) {
        if let Some(to_thread) = &self.shared.to_thread {
            let _ = to_thread.send(task);
        }
    }
}

/// The thread's state.
struct LocalStorage {
    profile: Profile,
    /// Opened as it is first needed: the profile's, or one in memory if
    /// that couldn't be opened.
    database: Option<Database>,
    /// Each area's items, once a renderer opened it.
    areas: HashMap<AreaKey, BTreeMap<String, String>>,
    /// Each renderer's areas, by number and by place, and its way.
    renderers: HashMap<u64, RendererAreas>,
    /// The areas' numbers, from 1.
    area_ids: SequenceNumber<u64>,
}

struct RendererAreas {
    to_renderer: ToRenderer,
    by_key: HashMap<AreaKey, u64>,
    keys: HashMap<u64, AreaKey>,
}

impl LocalStorage {
    fn new(profile: Profile) -> Self {
        Self {
            profile,
            database: None,
            areas: HashMap::new(),
            renderers: HashMap::new(),
            area_ids: SequenceNumber::starting_at(1),
        }
    }

    fn run(mut self, tasks: &Receiver<Task>) {
        while let Ok(task) = tasks.recv() {
            match task {
                Task::Open {
                    renderer,
                    to_renderer,
                    area,
                    request_id,
                } => {
                    let opened = self.open(renderer, to_renderer.clone(), area);
                    to_renderer(RendererMsg::StorageAreaOpened {
                        request_id,
                        area: opened,
                    });
                }
                Task::Change {
                    renderer,
                    area,
                    change,
                    source,
                    url,
                } => self.change(renderer, area, change, source, url),
                Task::Forget { renderer } => {
                    self.renderers.remove(&renderer);
                }
                Task::ForgetNamespace { namespace } => self.forget_namespace(namespace),
            }
        }
    }

    fn open(
        &mut self,
        renderer: u64,
        to_renderer: ToRenderer,
        place: AreaKey,
    ) -> Option<OpenedStorageArea> {
        if !place.key.origin().is_tuple() {
            return None;
        }
        let areas = self
            .renderers
            .entry(renderer)
            .or_insert_with(|| RendererAreas {
                to_renderer: to_renderer.clone(),
                by_key: HashMap::new(),
                keys: HashMap::new(),
            });
        areas.to_renderer = to_renderer;
        if let Some(&area) = areas.by_key.get(&place) {
            return Some(OpenedStorageArea { area, items: None });
        }
        let area = self.area_ids.get_next();
        areas.by_key.insert(place.clone(), area);
        areas.keys.insert(area, place.clone());
        if !self.areas.contains_key(&place) {
            let items = self.read(&place);
            self.areas.insert(place.clone(), items);
        }
        let items = self.areas.get(&place).map_or_else(Vec::new, |items| {
            items
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        });
        Some(OpenedStorageArea {
            area,
            items: Some(items),
        })
    }

    fn change(
        &mut self,
        renderer: u64,
        area: u64,
        change: StorageChange,
        source: u64,
        url: String,
    ) {
        let Some(place) = self
            .renderers
            .get(&renderer)
            .and_then(|areas| areas.keys.get(&area))
            .cloned()
        else {
            return;
        };
        let items = self.areas.entry(place.clone()).or_default();
        let (old_value, changed) = match &change {
            StorageChange::Set { key, value } => {
                let old = items.insert(key.clone(), value.clone());
                let changed = old.as_ref() != Some(value);
                (old, changed)
            }
            StorageChange::Remove { key } => {
                let old = items.remove(key);
                let changed = old.is_some();
                (old, changed)
            }
            StorageChange::Clear => {
                let changed = !items.is_empty();
                items.clear();
                (None, changed)
            }
        };
        if place.namespace.is_some() {
            // `sessionStorage`'s events are the renderer's: the tab's
            // documents are all in it.
            return;
        }
        self.write(&place, &change);
        // Every renderer with the area, the one that made the change too
        // (its `Storage` told, to skip): each fires `storage` at its
        // documents of the area (`StorageAreaObserver`).
        for (&other, areas) in &self.renderers {
            let Some(&their_area) = areas.by_key.get(&place) else {
                continue;
            };
            (areas.to_renderer)(RendererMsg::StorageAreaChanged {
                area: their_area,
                change: change.clone(),
                old_value: old_value.clone(),
                changed,
                url: url.clone(),
                source: (other == renderer).then_some(source),
            });
        }
    }

    fn forget_namespace(&mut self, namespace: TabId) {
        self.areas
            .retain(|place, _| place.namespace != Some(namespace));
        for areas in self.renderers.values_mut() {
            areas
                .by_key
                .retain(|place, _| place.namespace != Some(namespace));
            areas
                .keys
                .retain(|_, place| place.namespace != Some(namespace));
        }
    }

    /// `place`'s items on disk; none for one that isn't kept there (a
    /// `sessionStorage`'s, an opaque top-level site's).
    fn read(&mut self, place: &AreaKey) -> BTreeMap<String, String> {
        if place.namespace.is_some() {
            return BTreeMap::new();
        }
        let Some(serialized) = place.key.serialize() else {
            return BTreeMap::new();
        };
        let connection = self.database().connection();
        let read = connection
            .prepare_cached("SELECT key, value FROM items WHERE storage_key = ?1")
            .and_then(|mut statement| {
                statement
                    .query_map([&serialized], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect()
            });
        read.unwrap_or_else(|error| {
            tracing::warn!(%error, "couldn't read localStorage");
            BTreeMap::new()
        })
    }

    fn write(&mut self, place: &AreaKey, change: &StorageChange) {
        let Some(serialized) = place.key.serialize() else {
            return;
        };
        let connection = self.database().connection();
        let written = match change {
            StorageChange::Set { key, value } => connection.execute(
                "INSERT OR REPLACE INTO items (storage_key, key, value) VALUES (?1, ?2, ?3)",
                params![serialized, key, value],
            ),
            StorageChange::Remove { key } => connection.execute(
                "DELETE FROM items WHERE storage_key = ?1 AND key = ?2",
                params![serialized, key],
            ),
            StorageChange::Clear => connection.execute(
                "DELETE FROM items WHERE storage_key = ?1",
                params![serialized],
            ),
        };
        if let Err(error) = written {
            tracing::warn!(%error, "couldn't write localStorage");
        }
    }

    /// The database, opened the first time: the profile's, or one in
    /// memory if that can't be opened (what is stored lasts as long as
    /// this machopo, as Chromium's does when its database fails).
    fn database(&mut self) -> &mut Database {
        let dir = self.profile.dir();
        self.database.get_or_insert_with(|| {
            let opened = Database::open(dir, DATABASE).and_then(|mut database| {
                database.migrate(MIGRATIONS)?;
                Ok(database)
            });
            opened.unwrap_or_else(|error| {
                tracing::warn!(%error, "couldn't open localStorage's database: kept in memory");
                let mut database =
                    Database::open(None, DATABASE).expect("a database in memory opens");
                database
                    .migrate(MIGRATIONS)
                    .expect("a new database in memory takes the schema");
                database
            })
        })
    }
}
