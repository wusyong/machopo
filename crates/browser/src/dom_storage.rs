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
use std::time::{Duration, Instant};

use base::SequenceNumber;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use protocol::TabId;
use protocol::renderer::{
    OpenedStorageArea, PER_STORAGE_AREA_OVER_QUOTA_ALLOWANCE, PER_STORAGE_AREA_QUOTA, RendererMsg,
    StorageChange, StorageKind, storage_quota_for,
};
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

/// When `localStorage`'s changes are written to disk (Chromium's
/// `StorageAreaImpl::Options`): each area's in a batch, a transaction,
/// once the longest of `delay` and what the area's rates allow has passed
/// (its commits and bytes in an hour, from its first change; 0 is no
/// limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitOptions {
    pub delay: Duration,
    pub max_commits_per_hour: u64,
    pub max_bytes_per_hour: u64,
}

impl Default for CommitOptions {
    /// Chromium's (`LocalStorageImpl`): 5 seconds, 60 commits and 10 MiB
    /// (`kPerStorageAreaQuota`) an hour.
    fn default() -> Self {
        Self {
            delay: Duration::from_secs(5),
            max_commits_per_hour: 60,
            max_bytes_per_hour: 10 * 1024 * 1024,
        }
    }
}

impl DomStorageContext {
    /// `profile`'s: `localStorage` on disk in its folder, if it has one
    /// (the database opened as it is first needed), else in memory.
    pub fn new(profile: &Profile) -> std::io::Result<Self> {
        Self::with_commit_options(profile, CommitOptions::default())
    }

    /// [`new`](Self::new), its changes written to disk as `options` say.
    pub fn with_commit_options(profile: &Profile, options: CommitOptions) -> std::io::Result<Self> {
        let (to_thread, tasks) = crossbeam_channel::unbounded();
        let profile = profile.clone();
        let thread = std::thread::Builder::new()
            .name("LocalStorage".into())
            .spawn(move || LocalStorage::new(profile, options).run(&tasks))?;
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
    areas: HashMap<AreaKey, Stored>,
    /// Each renderer's areas, by number and by place, and its way.
    renderers: HashMap<u64, RendererAreas>,
    /// The areas' numbers, from 1.
    area_ids: SequenceNumber<u64>,
    options: CommitOptions,
    /// Each `localStorage` area's changes not on disk yet, by its key as
    /// written, and when they are due.
    batches: HashMap<String, Batch>,
    /// Each `localStorage` area's commits so far (`RateLimiter`'s).
    rates: HashMap<String, CommitRate>,
}

struct Batch {
    changes: Vec<StorageChange>,
    due: Instant,
}

/// An area's commits and the bytes they wrote since its first change
/// (Chromium's two `RateLimiter`s of a `StorageAreaImpl`).
struct CommitRate {
    start: Instant,
    commits: u64,
    bytes: u64,
}

/// How long a rate of `max` an hour takes for `samples` (Chromium's
/// `RateLimiter::ComputeTimeNeeded`), whole seconds; none without a limit.
fn time_needed(samples: u64, max: u64) -> Duration {
    Duration::from_secs(
        3600u64
            .saturating_mul(samples)
            .checked_div(max)
            .unwrap_or(0),
    )
}

/// How long to wait from `now` to commit an area's batch: the longest of
/// `options`' delay and what its rate allows, `elapsed` after its first
/// change (Chromium's `StorageAreaImpl::ComputeCommitDelay`).
fn commit_delay(options: &CommitOptions, rate: &CommitRate, elapsed: Duration) -> Duration {
    let commits = time_needed(rate.commits, options.max_commits_per_hour).saturating_sub(elapsed);
    let bytes = time_needed(rate.bytes, options.max_bytes_per_hour).saturating_sub(elapsed);
    options.delay.max(commits).max(bytes)
}

/// An area's items, and the bytes they take of its quota
/// (`storage_quota_for`).
#[derive(Default)]
struct Stored {
    items: BTreeMap<String, String>,
    used: usize,
}

impl Stored {
    fn new(items: BTreeMap<String, String>) -> Self {
        let used = items
            .iter()
            .map(|(key, value)| storage_quota_for(key, value))
            .fold(0, usize::saturating_add);
        Self { items, used }
    }

    /// Whether `key` may be `value`: always if the item doesn't grow, else
    /// within the quota and the allowance (several renderers write at
    /// once; Chromium's `StorageAreaImpl::Put`).
    fn fits(&self, key: &str, value: &str) -> bool {
        let old = self
            .items
            .get(key)
            .map_or(0, |old| storage_quota_for(key, old));
        let new = storage_quota_for(key, value);
        new <= old
            || self.used.saturating_sub(old).saturating_add(new)
                <= PER_STORAGE_AREA_QUOTA.saturating_add(PER_STORAGE_AREA_OVER_QUOTA_ALLOWANCE)
    }

    fn insert(&mut self, key: &str, value: &str) -> Option<String> {
        let old = self.items.insert(key.to_owned(), value.to_owned());
        if let Some(old) = &old {
            self.used = self.used.saturating_sub(storage_quota_for(key, old));
        }
        self.used = self.used.saturating_add(storage_quota_for(key, value));
        old
    }

    fn remove(&mut self, key: &str) -> Option<String> {
        let old = self.items.remove(key)?;
        self.used = self.used.saturating_sub(storage_quota_for(key, &old));
        Some(old)
    }

    fn clear(&mut self) {
        self.items.clear();
        self.used = 0;
    }
}

struct RendererAreas {
    to_renderer: ToRenderer,
    by_key: HashMap<AreaKey, u64>,
    keys: HashMap<u64, AreaKey>,
}

impl LocalStorage {
    fn new(profile: Profile, options: CommitOptions) -> Self {
        Self {
            profile,
            database: None,
            areas: HashMap::new(),
            renderers: HashMap::new(),
            area_ids: SequenceNumber::starting_at(1),
            options,
            batches: HashMap::new(),
            rates: HashMap::new(),
        }
    }

    fn run(mut self, tasks: &Receiver<Task>) {
        loop {
            let next_due = self.batches.values().map(|batch| batch.due).min();
            let task = match next_due {
                None => tasks.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(due) => tasks.recv_timeout(due.saturating_duration_since(Instant::now())),
            };
            let task = match task {
                Ok(task) => task,
                Err(RecvTimeoutError::Timeout) => {
                    self.commit_due(Instant::now());
                    continue;
                }
                // Every handle gone (machopo closing): what is left is
                // written now (`ScheduleImmediateCommit`).
                Err(RecvTimeoutError::Disconnected) => {
                    let keys: Vec<String> = self.batches.keys().cloned().collect();
                    for key in keys {
                        self.commit(&key);
                    }
                    return;
                }
            };
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
            self.commit_due(Instant::now());
        }
    }

    /// The batches due by `now`, written.
    fn commit_due(&mut self, now: Instant) {
        let due: Vec<String> = self
            .batches
            .iter()
            .filter(|(_, batch)| batch.due <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            self.commit(&key);
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
            self.areas.insert(place.clone(), Stored::new(items));
        }
        let items = self.areas.get(&place).map_or_else(Vec::new, |stored| {
            stored
                .items
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
                if !items.fits(key, value) {
                    // The renderer's copy goes back to the browser's.
                    let value = items.items.get(key).cloned();
                    if let Some(areas) = self.renderers.get(&renderer) {
                        (areas.to_renderer)(RendererMsg::StorageAreaChangeFailed {
                            area,
                            key: key.clone(),
                            value,
                        });
                    }
                    return;
                }
                let old = items.insert(key, value);
                let changed = old.as_ref() != Some(value);
                (old, changed)
            }
            StorageChange::Remove { key } => {
                let old = items.remove(key);
                let changed = old.is_some();
                (old, changed)
            }
            StorageChange::Clear => {
                let changed = !items.items.is_empty();
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

    /// `change` of a `localStorage` area kept on disk, in its area's batch,
    /// which is due once its delay has passed from the first change in it.
    fn write(&mut self, place: &AreaKey, change: &StorageChange) {
        let Some(serialized) = place.key.serialize() else {
            return;
        };
        let now = Instant::now();
        if !self.batches.contains_key(&serialized) {
            let rate = self.rates.entry(serialized.clone()).or_insert(CommitRate {
                start: now,
                commits: 0,
                bytes: 0,
            });
            let delay = commit_delay(
                &self.options,
                rate,
                now.saturating_duration_since(rate.start),
            );
            self.batches.insert(
                serialized.clone(),
                Batch {
                    changes: Vec::new(),
                    // A wait too long to count: an hour (or none).
                    due: now
                        .checked_add(delay)
                        .or_else(|| now.checked_add(Duration::from_secs(3600)))
                        .unwrap_or(now),
                },
            );
        }
        if let Some(batch) = self.batches.get_mut(&serialized) {
            batch.changes.push(change.clone());
        }
    }

    /// The batch of the area written as `serialized`, written in one
    /// transaction (Chromium's `StorageAreaImpl::CommitChanges`).
    fn commit(&mut self, serialized: &str) {
        let Some(batch) = self.batches.remove(serialized) else {
            return;
        };
        let bytes = batch
            .changes
            .iter()
            .map(|change| match change {
                StorageChange::Set { key, value } => storage_quota_for(key, value),
                StorageChange::Remove { key } => storage_quota_for(key, ""),
                StorageChange::Clear => 0,
            })
            .fold(0, usize::saturating_add);
        if let Some(rate) = self.rates.get_mut(serialized) {
            rate.commits = rate.commits.saturating_add(1);
            rate.bytes = rate
                .bytes
                .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        }
        let connection = self.database().connection();
        let written = connection.transaction().and_then(|transaction| {
            for change in &batch.changes {
                match change {
                    StorageChange::Set { key, value } => transaction.execute(
                        "INSERT OR REPLACE INTO items (storage_key, key, value) VALUES (?1, ?2, ?3)",
                        params![serialized, key, value],
                    ),
                    StorageChange::Remove { key } => transaction.execute(
                        "DELETE FROM items WHERE storage_key = ?1 AND key = ?2",
                        params![serialized, key],
                    ),
                    StorageChange::Clear => transaction.execute(
                        "DELETE FROM items WHERE storage_key = ?1",
                        params![serialized],
                    ),
                }?;
            }
            transaction.commit()
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch waits the delay at least; once the area has committed
    /// often, or written much, in its first hour, longer: the rest of the
    /// time its rate takes for them (Chromium's `RateLimiter`).
    #[test]
    fn a_batch_waits_for_its_areas_rate() {
        let options = CommitOptions::default();
        let rate = |commits, bytes| CommitRate {
            start: Instant::now(),
            commits,
            bytes,
        };
        let secs = Duration::from_secs;
        assert_eq!(commit_delay(&options, &rate(0, 0), secs(0)), secs(5));
        // A commit a minute is the rate: the second waits out the first's.
        assert_eq!(commit_delay(&options, &rate(1, 0), secs(10)), secs(50));
        assert_eq!(commit_delay(&options, &rate(1, 0), secs(58)), secs(5));
        assert_eq!(commit_delay(&options, &rate(30, 0), secs(600)), secs(1200));
        // 10 MiB an hour: half of it written, half an hour's wait.
        assert_eq!(
            commit_delay(&options, &rate(1, 5 * 1024 * 1024), secs(60)),
            secs(1740)
        );
        // No limits: the delay alone.
        let unlimited = CommitOptions {
            delay: Duration::ZERO,
            max_commits_per_hour: 0,
            max_bytes_per_hour: 0,
        };
        assert_eq!(
            commit_delay(&unlimited, &rate(1000, 1 << 30), secs(0)),
            Duration::ZERO
        );
    }
}
