//! The cache's entries on disk (Chromium's SQL disk cache backend,
//! `SqlBackendImpl`): what a profile in a folder has. The entries are the
//! store's, on its thread; what is known of them here is its index in
//! memory (`SqlPersistentStoreInMemoryIndex`): each complete entry's size
//! and when it was last used, read in as the store opens. A key the index
//! doesn't have is a miss at once; one it has is read on the store's
//! thread.
//!
//! Its size is bounded by what it was opened with: unless told, what
//! Chromium's `PreferredCacheSize` makes of what the disk has free
//! ([`preferred_size_for`]). One entry may take an eighth of it, or 5 MiB if
//! that is more. Past 95% of the bound, entries
//! go until it is down to 90%, those longest unused and biggest first: by
//! how long since each was used times its size and 300 bytes (Chromium's
//! `CalculateSortValue`, Simple Cache's alike), so of two as old the bigger
//! goes, of two as big the older.

use std::collections::HashMap;
use std::path::Path;

use bytes::Bytes;
use tokio::sync::oneshot;

use super::entry::StoredResponse;
use super::sql_store::{IndexEntry, SqlStore};
use crate::cookies::Time;

/// Chromium's `kDefaultCacheSize`.
const DEFAULT_CACHE_SIZE: u64 = 80 * 1024 * 1024;

/// How `PreferredCacheSize` scales the HTTP cache's (`DISK_CACHE`'s) size:
/// fourfold, but on Windows.
const SCALE_PERCENT: u64 = if cfg!(windows) { 100 } else { 400 };

/// What a cache in the folder `dir` is bounded at unless told
/// (`PreferredCacheSizeForPath`): as [`preferred_size`] has it for what
/// the disk `dir` is on has free. The folder is made, if it isn't there,
/// to ask.
pub(crate) fn preferred_size_for(dir: &Path) -> u64 {
    let _ = std::fs::create_dir_all(dir);
    preferred_size(ipc::platform::available_disk_space(dir).ok(), SCALE_PERCENT)
}

/// Chromium's `PreferredCacheSize` for a disk with `available` bytes free
/// (none: it couldn't be read), its size scaled by `percent`: for little
/// room, most of it; then 80 MiB; then a tenth, 200 MiB, a hundredth
/// (`PreferredCacheSizeInternal`); scaled, but to no more than a fifth of
/// what is free; and never more than four times the scaled 80 MiB.
fn preferred_size(available: Option<u64>, percent: u64) -> u64 {
    let scaled_default = DEFAULT_CACHE_SIZE.saturating_mul(percent) / 100;
    let preferred = match available {
        None => scaled_default,
        Some(available) => {
            let preferred = preferred_size_internal(available);
            let fifth = available / 5;
            if preferred < fifth {
                (preferred.saturating_mul(percent) / 100).min(fifth)
            } else {
                preferred
            }
        }
    };
    preferred.min(scaled_default.saturating_mul(4))
}

fn preferred_size_internal(available: u64) -> u64 {
    const LITTLE: u64 = DEFAULT_CACHE_SIZE * 10 / 8;
    const ENOUGH: u64 = DEFAULT_CACHE_SIZE * 10;
    const MORE: u64 = DEFAULT_CACHE_SIZE * 25;
    const PLENTY: u64 = DEFAULT_CACHE_SIZE * 250;
    if available < LITTLE {
        // 80% of it, which fits.
        u64::try_from(u128::from(available) * 8 / 10).unwrap_or(available)
    } else if available < ENOUGH {
        DEFAULT_CACHE_SIZE
    } else if available < MORE {
        available / 10
    } else if available < PLENTY {
        DEFAULT_CACHE_SIZE * 5 / 2
    } else {
        available / 100
    }
}

/// What an entry may take at least, whatever the bound
/// (`kSqlBackendMinFileSizeLimit`).
const MIN_MAX_ENTRY_SIZE: u64 = 5 * 1024 * 1024;

/// What an entry counts beyond its size when picking which go
/// (`kSqlBackendStaticResourceSize`).
const ENTRY_OVERHEAD: u64 = 300;

pub(crate) struct DiskBackend {
    store: SqlStore,
    max_size: u64,
    size: u64,
    index: HashMap<String, Indexed>,
}

struct Indexed {
    size: u64,
    last_used: Time,
}

impl DiskBackend {
    /// The entries of `store` (none until [`Self::load`]), bounded at
    /// `max_size` bytes.
    pub(crate) fn new(store: SqlStore, max_size: u64) -> Self {
        Self {
            store,
            max_size,
            size: 0,
            index: HashMap::new(),
        }
    }

    /// The complete entries the store read as it opened, past the bound's
    /// evicted.
    pub(crate) fn load(&mut self, entries: Vec<IndexEntry>, now: Time) {
        for entry in entries {
            self.set(entry.key, entry.size, entry.last_used);
        }
        self.evict(now);
    }

    /// The most one entry may take (`MaxFileSize`).
    pub(crate) fn max_entry_size(&self) -> u64 {
        (self.max_size / 8).max(MIN_MAX_ENTRY_SIZE)
    }

    /// `key`'s entry, now used, as the store reads it; none if there is
    /// none.
    pub(crate) fn read(
        &mut self,
        key: &str,
        now: Time,
    ) -> Option<oneshot::Receiver<Option<StoredResponse>>> {
        let indexed = self.index.get_mut(key)?;
        indexed.last_used = now;
        self.store.touch(key, now);
        Some(self.store.read(key))
    }

    /// A new entry for `key` begins: `head`, its body to come.
    pub(crate) fn begin(&self, key: &str, head: &StoredResponse) {
        self.store.begin(
            key,
            StoredResponse {
                body: Vec::new(),
                ..head.clone()
            },
        );
    }

    pub(crate) fn append(&self, key: &str, piece: Bytes) {
        self.store.append(key, piece);
    }

    pub(crate) fn abandon(&self, key: &str) {
        self.store.abandon(key);
    }

    /// `key`'s new entry, `size` bytes, is complete: it replaces the one
    /// before.
    pub(crate) fn finish(&mut self, key: &str, size: u64, now: Time) {
        self.store.finish(key, now);
        self.set(key.to_owned(), size, now);
        self.evict(now);
    }

    /// `key`'s entry has `response`'s head (a `304`'s), its body as it was;
    /// one grown too big goes.
    pub(crate) fn update(&mut self, key: &str, response: StoredResponse, now: Time) {
        if !self.index.contains_key(key) {
            return;
        }
        let size = response.size();
        if size > self.max_entry_size() {
            self.remove(key);
            return;
        }
        self.store.update_head(
            key,
            StoredResponse {
                body: Vec::new(),
                ..response
            },
            size,
            now,
        );
        self.set(key.to_owned(), size, now);
        self.evict(now);
    }

    /// `key`'s entry goes (`DoomEntry`).
    pub(crate) fn remove(&mut self, key: &str) {
        if self.forget(key) {
            self.store.remove(key);
        }
    }

    /// `key`'s entry is no longer known here (the store couldn't read it);
    /// whether it was.
    pub(crate) fn forget(&mut self, key: &str) -> bool {
        let Some(gone) = self.index.remove(key) else {
            return false;
        };
        self.size = self.size.saturating_sub(gone.size);
        true
    }

    /// Every entry goes, those being written too; then `done`.
    pub(crate) fn clear(&mut self, done: impl FnOnce() + Send + 'static) {
        self.razed();
        self.store.clear(done);
    }

    /// The store started over: none of what was known is there.
    pub(crate) fn razed(&mut self) {
        self.index.clear();
        self.size = 0;
    }

    /// What its entries take, in bytes.
    #[cfg(test)]
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    #[cfg(test)]
    pub(crate) fn has(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    fn set(&mut self, key: String, size: u64, last_used: Time) {
        self.size = self.size.saturating_add(size);
        if let Some(old) = self.index.insert(key, Indexed { size, last_used }) {
            self.size = self.size.saturating_sub(old.size);
        }
    }

    /// Past 95% of the bound, entries go until it is down to 90%, by how
    /// long unused times how big (`StartEvictionIfNeeded`,
    /// `kSqlBackendEvictionHighWaterMarkPermille`).
    fn evict(&mut self, now: Time) {
        let twentieth = self.max_size / 20;
        if self.size <= self.max_size.saturating_sub(twentieth) {
            return;
        }
        let low = self.max_size.saturating_sub(twentieth.saturating_mul(2));
        let mut by_score: Vec<(u128, &String, u64)> = self
            .index
            .iter()
            .map(|(key, indexed)| (score(indexed, now), key, indexed.size))
            .collect();
        // The highest first; of the same score, by key, so it is always the
        // same.
        by_score.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let mut size = self.size;
        let mut gone = Vec::new();
        for (_, key, entry_size) in by_score {
            if size <= low {
                break;
            }
            size = size.saturating_sub(entry_size);
            gone.push(key.clone());
        }
        for key in gone {
            self.remove(&key);
        }
    }
}

/// How much an entry ought to go: the microseconds since it was last used
/// times its size and [`ENTRY_OVERHEAD`]. One used later than now (the
/// clock went back) counts as just used.
fn score(indexed: &Indexed, now: Time) -> u128 {
    let age = now
        .unix_micros()
        .saturating_sub(indexed.last_used.unix_micros());
    let age = u128::try_from(age).unwrap_or(0);
    age.saturating_mul(u128::from(indexed.size.saturating_add(ENTRY_OVERHEAD)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::http_cache::sql_store::SqlStoreThread;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "machopo-http-cache-disk-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn backend(name: &str, max_size: u64) -> (DiskBackend, SqlStoreThread) {
        let (store, thread, loaded) = SqlStore::open(&scratch(name), || {}).unwrap();
        assert_eq!(loaded.blocking_recv().unwrap(), []);
        (DiskBackend::new(store, max_size), thread)
    }

    fn entry(key: &str, size: u64, last_used: i64) -> IndexEntry {
        IndexEntry {
            key: key.to_owned(),
            size,
            last_used: Time::from_unix_micros(last_used),
        }
    }

    const NOW: Time = Time::from_unix_micros(1_000_000);

    #[test]
    fn of_two_the_one_long_unused_times_big_goes_first() {
        let (mut disk, _thread) = backend("score", 1000);
        // Scores: 1e6 * 310, 6e5 * 900, 0. Least recently used would take
        // the small old one first.
        disk.load(
            vec![
                entry("old-small", 10, 0),
                entry("newer-big", 600, 400_000),
                entry("fresh", 400, 1_000_000),
            ],
            NOW,
        );
        assert!(!disk.has("newer-big"));
        assert!(disk.has("old-small"));
        assert!(disk.has("fresh"));
        assert_eq!(disk.size(), 410);
    }

    #[test]
    fn past_95_percent_it_goes_down_to_90() {
        let (mut disk, _thread) = backend("watermarks", 1000);
        // 950: not past it.
        disk.load(vec![entry("a", 40, 0), entry("b", 890, 900_000)], NOW);
        disk.load(vec![entry("c", 20, 999_000)], NOW);
        assert_eq!(disk.size(), 950);
        // 960: `a` goes first (1e6 * 340, against 1e5 * 1190 and 1e3 *
        // 320), leaving 920, under 95% but past 90%: `b` goes too.
        disk.load(vec![entry("d", 10, 1_000_000)], NOW);
        assert!(!disk.has("a"));
        assert!(!disk.has("b"));
        assert!(disk.has("c"));
        assert!(disk.has("d"));
        assert_eq!(disk.size(), 30);
    }

    fn head() -> StoredResponse {
        StoredResponse {
            status: 200,
            status_text: String::new(),
            protocol: "http/1.1",
            headers: Vec::new(),
            request_time: Time::UNIX_EPOCH,
            response_time: Time::UNIX_EPOCH,
            vary: None,
            body: Vec::new(),
        }
    }

    #[test]
    fn a_304_that_grows_an_entry_evicts() {
        let (mut disk, _thread) = backend("304", 1000);
        disk.load(vec![entry("old", 100, 0), entry("b", 100, 1_000_000)], NOW);
        let grown = StoredResponse {
            headers: vec![("x".to_owned(), "x".repeat(899))],
            ..head()
        };
        disk.update("b", grown, NOW);
        assert!(!disk.has("old"));
        assert_eq!(disk.size(), 900);
    }

    #[test]
    fn a_read_is_a_use_and_is_written() {
        let dir = scratch("used");
        let (store, thread, _) = SqlStore::open(&dir, || {}).unwrap();
        let mut disk = DiskBackend::new(store, 1000);
        for key in ["a", "b"] {
            disk.begin(key, &head());
            disk.finish(key, 400, Time::UNIX_EPOCH);
        }
        // Read, `a` is newer than `b`: `b` goes for `c`.
        let _ = disk.read("a", NOW);
        disk.load(vec![entry("c", 200, 1_000_000)], NOW);
        assert!(disk.has("a"));
        assert!(!disk.has("b"));
        assert!(disk.read("b", NOW).is_none());
        drop(thread);
        // And kept so.
        let (_store, _thread, loaded) = SqlStore::open(&dir, || {}).unwrap();
        let used: Vec<_> = loaded
            .blocking_recv()
            .unwrap()
            .into_iter()
            .map(|entry| (entry.key, entry.last_used))
            .collect();
        assert_eq!(used, [("a".to_owned(), NOW)]);
    }

    #[test]
    fn an_entry_takes_an_eighth_or_5_mib() {
        let (disk, _thread) = backend("entry-size", 80 * 1024 * 1024);
        assert_eq!(disk.max_entry_size(), 10 * 1024 * 1024);
        let (disk, _thread) = backend("entry-size-small", 1000);
        assert_eq!(disk.max_entry_size(), 5 * 1024 * 1024);
    }

    #[test]
    fn its_size_follows_its_entries() {
        let (mut disk, _thread) = backend("sizes", 1_000_000);
        disk.load(vec![entry("a", 100, 0), entry("b", 200, 0)], NOW);
        assert_eq!(disk.size(), 300);
        // Replaced, the old size goes.
        disk.finish("a", 50, NOW);
        assert_eq!(disk.size(), 250);
        disk.remove("b");
        assert_eq!(disk.size(), 50);
        assert!(!disk.forget("b"));
        assert!(disk.forget("a"));
        assert_eq!(disk.size(), 0);
        disk.load(vec![entry("c", 1, 0)], NOW);
        disk.razed();
        assert_eq!((disk.size(), disk.has("c")), (0, false));
    }

    #[test]
    fn a_clock_gone_back_counts_as_just_used() {
        let future = Indexed {
            size: 10,
            last_used: Time::from_unix_micros(2_000_000),
        };
        assert_eq!(score(&future, NOW), 0);
        let past = Indexed {
            size: 10,
            last_used: Time::from_unix_micros(0),
        };
        assert_eq!(score(&past, NOW), 1_000_000 * 310);
    }
}

#[cfg(test)]
mod preferred_size_tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn as_chromium_has_it_outside_windows() {
        let size = |available_mib: u64| preferred_size(Some(available_mib * MIB), 400);
        assert_eq!(size(50), 40 * MIB);
        assert_eq!(size(100), 80 * MIB);
        assert_eq!(size(400), 80 * MIB);
        assert_eq!(size(600), 120 * MIB);
        assert_eq!(size(1000), 200 * MIB);
        assert_eq!(size(3000), 600 * MIB);
        assert_eq!(size(4000), 800 * MIB);
        assert_eq!(size(19_000), 800 * MIB);
        assert_eq!(size(25_000), 1000 * MIB);
        assert_eq!(size(500_000), 1280 * MIB);
        assert_eq!(preferred_size(None, 400), 320 * MIB);
    }

    #[test]
    fn as_chromium_has_it_on_windows() {
        let size = |available_mib: u64| preferred_size(Some(available_mib * MIB), 100);
        assert_eq!(size(50), 40 * MIB);
        assert_eq!(size(600), 80 * MIB);
        assert_eq!(size(1000), 100 * MIB);
        assert_eq!(size(3000), 200 * MIB);
        assert_eq!(size(25_000), 250 * MIB);
        assert_eq!(size(500_000), 320 * MIB);
        assert_eq!(preferred_size(None, 100), 80 * MIB);
    }

    #[test]
    fn a_folders_size_goes_by_its_disk() {
        let dir = std::env::temp_dir().join(format!(
            "machopo-http-cache-size-{}/made",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let size = preferred_size_for(&dir);
        assert!(dir.is_dir());
        let available = ipc::platform::available_disk_space(&dir).ok();
        assert!(available.is_some());
        // Fourfold, but on Windows.
        let percent = if cfg!(windows) { 100 } else { 400 };
        assert_eq!(size, preferred_size(available, percent));
    }
}
