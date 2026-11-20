//! `localStorage`'s areas (Chromium's `LocalStorageImpl` in the storage
//! service, `DOMStorageContextWrapper` the browser's handle on it): each
//! storage key's items, kept on a thread of their own and in the profile's
//! `LocalStorage` database, for the renderers that opened them. A renderer
//! gets each area once, with its items, and keeps its own copy (Blink's
//! `CachedStorageArea`): what it changes it changes first, then tells.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::thread::JoinHandle;

use base::SequenceNumber;
use crossbeam_channel::{Receiver, Sender};
use protocol::renderer::{OpenedStorageArea, StorageChange};
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

/// What answers an open: the area, or none.
pub type OpenReply = Box<dyn FnOnce(Option<OpenedStorageArea>) + Send>;

/// The browser's handle on the profile's `localStorage`: clones share the
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

enum Task {
    Open {
        renderer: u64,
        key: StorageKey,
        reply: OpenReply,
    },
    Change {
        renderer: u64,
        area: u64,
        change: StorageChange,
    },
    Forget {
        renderer: u64,
    },
}

impl DomStorageContext {
    /// `profile`'s: on disk in its folder, if it has one (the database
    /// opened as it is first needed), else in memory.
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

    /// Renderer `renderer`'s frame of `key` opens its area: `reply` gets
    /// it, with its items unless the renderer has it already, or none for
    /// an opaque origin's key (no storage, HTML's `SecurityError`).
    pub fn open(&self, renderer: u64, key: StorageKey, reply: OpenReply) {
        self.send(Task::Open {
            renderer,
            key,
            reply,
        });
    }

    /// Renderer `renderer` changed its area `area`: nothing if it isn't
    /// one the renderer opened.
    pub fn change(&self, renderer: u64, area: u64, change: StorageChange) {
        self.send(Task::Change {
            renderer,
            area,
            change,
        });
    }

    /// Renderer `renderer` has none of its areas any more (it has gone, or
    /// its DOM did): it opens them again.
    pub fn forget(&self, renderer: u64) {
        self.send(Task::Forget { renderer });
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
    /// Each key's items, once a renderer opened it.
    areas: HashMap<StorageKey, BTreeMap<String, String>>,
    /// Each renderer's areas, by number and by key.
    renderers: HashMap<u64, RendererAreas>,
    /// The areas' numbers, from 1.
    area_ids: SequenceNumber<u64>,
}

#[derive(Default)]
struct RendererAreas {
    by_key: HashMap<StorageKey, u64>,
    keys: HashMap<u64, StorageKey>,
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
                    key,
                    reply,
                } => reply(self.open(renderer, key)),
                Task::Change {
                    renderer,
                    area,
                    change,
                } => self.change(renderer, area, change),
                Task::Forget { renderer } => {
                    self.renderers.remove(&renderer);
                }
            }
        }
    }

    fn open(&mut self, renderer: u64, key: StorageKey) -> Option<OpenedStorageArea> {
        if !key.origin().is_tuple() {
            return None;
        }
        let areas = self.renderers.entry(renderer).or_default();
        if let Some(&area) = areas.by_key.get(&key) {
            return Some(OpenedStorageArea { area, items: None });
        }
        let area = self.area_ids.get_next();
        areas.by_key.insert(key.clone(), area);
        areas.keys.insert(area, key.clone());
        if !self.areas.contains_key(&key) {
            let items = self.read(&key);
            self.areas.insert(key.clone(), items);
        }
        let items = self.areas.get(&key).map_or_else(Vec::new, |items| {
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

    fn change(&mut self, renderer: u64, area: u64, change: StorageChange) {
        let Some(key) = self
            .renderers
            .get(&renderer)
            .and_then(|areas| areas.keys.get(&area))
            .cloned()
        else {
            return;
        };
        let items = self.areas.entry(key.clone()).or_default();
        match &change {
            StorageChange::Set { key, value } => {
                items.insert(key.clone(), value.clone());
            }
            StorageChange::Remove { key } => {
                items.remove(key);
            }
            StorageChange::Clear => items.clear(),
        }
        self.write(&key, &change);
    }

    /// `key`'s items on disk; none for a key that isn't kept there (an
    /// opaque top-level site's).
    fn read(&mut self, key: &StorageKey) -> BTreeMap<String, String> {
        let Some(serialized) = key.serialize() else {
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

    fn write(&mut self, key: &StorageKey, change: &StorageChange) {
        let Some(serialized) = key.serialize() else {
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
