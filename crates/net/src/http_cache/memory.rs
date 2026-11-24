//! The cache's entries in memory (Chromium's `disk_cache::MemBackendImpl`):
//! what a profile in memory has, and for now every profile.
//!
//! Its size is bounded: Chromium's is 2% of the machine's memory, 50 MiB at
//! 2.5 GiB and more, which is what this one always is (reading the
//! machine's memory is a system call, and every machine machopo runs on has
//! more). One entry may take an eighth of it; a bigger one isn't kept.
//! Past the bound, the least recently used go until it is down to 90%, as
//! Chromium evicts 10% more than it has to so as not to evict at every
//! write. An entry being read is a copy of its pieces (`Bytes` share them),
//! so one going doesn't take a read's body away; Chromium skips the entries
//! in use instead.

use std::collections::{BTreeMap, HashMap};

use super::entry::StoredResponse;

/// The cache's bound (`kMaxMemoryCacheSize`).
pub(crate) const MAX_SIZE: u64 = 50 * 1024 * 1024;

pub(crate) struct MemoryBackend {
    max_size: u64,
    size: u64,
    entries: HashMap<String, Stored>,
    /// The keys by when they were last used, oldest first.
    by_use: BTreeMap<u64, String>,
    /// What the next use is numbered: one a load, so 64 bits never run out.
    next_use: u64,
}

struct Stored {
    response: StoredResponse,
    size: u64,
    used: u64,
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::with_max_size(MAX_SIZE)
    }
}

impl MemoryBackend {
    /// One bounded at `max_size` bytes.
    pub(crate) fn with_max_size(max_size: u64) -> Self {
        Self {
            max_size,
            size: 0,
            entries: HashMap::new(),
            by_use: BTreeMap::new(),
            next_use: 0,
        }
    }

    /// The most one entry may take (`MaxFileSize`): an eighth of the bound.
    pub(crate) fn max_entry_size(&self) -> u64 {
        self.max_size / 8
    }

    /// The entry of `key`, now the most recently used.
    pub(crate) fn get(&mut self, key: &str) -> Option<&StoredResponse> {
        let used = self.take_use();
        let stored = self.entries.get_mut(key)?;
        self.by_use.remove(&stored.used);
        self.by_use.insert(used, key.to_owned());
        stored.used = used;
        Some(&stored.response)
    }

    /// Keeps `response` as `key`'s entry, in place of the one it had, if it
    /// isn't bigger than [`max_entry_size`](Self::max_entry_size); then
    /// evicts what it has to. Whether it was kept.
    pub(crate) fn put(&mut self, key: &str, response: StoredResponse) -> bool {
        // The key counts too, as Chromium's entries' do.
        let size = response
            .size()
            .saturating_add(u64::try_from(key.len()).unwrap_or(u64::MAX));
        self.remove(key);
        if size > self.max_entry_size() {
            return false;
        }
        let used = self.take_use();
        self.size = self.size.saturating_add(size);
        self.by_use.insert(used, key.to_owned());
        self.entries.insert(
            key.to_owned(),
            Stored {
                response,
                size,
                used,
            },
        );
        self.evict_if_needed();
        true
    }

    /// Drops `key`'s entry (`DoomEntry`), if it has one.
    pub(crate) fn remove(&mut self, key: &str) {
        if let Some(stored) = self.entries.remove(key) {
            self.by_use.remove(&stored.used);
            self.size = self.size.saturating_sub(stored.size);
        }
    }

    /// Drops every entry (`DoomAllEntries`).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "DevTools' clearing the cache calls it, in a later step")
    )]
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.by_use.clear();
        self.size = 0;
    }

    /// The bytes its entries take.
    #[cfg(test)]
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    fn take_use(&mut self) -> u64 {
        let used = self.next_use;
        self.next_use = self.next_use.saturating_add(1);
        used
    }

    /// Past its bound, the least recently used entries go until it is down
    /// to 90% of it (`EvictIfNeeded`, `EvictTill`).
    fn evict_if_needed(&mut self) {
        if self.size <= self.max_size {
            return;
        }
        let target = self.max_size.saturating_sub(self.max_size / 10);
        while self.size > target {
            let Some((_, key)) = self.by_use.pop_first() else {
                break;
            };
            if let Some(stored) = self.entries.remove(&key) {
                self.size = self.size.saturating_sub(stored.size);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;
    use crate::cookies::Time;

    /// A response whose size is `body` bytes and nothing else.
    fn response(body: usize) -> StoredResponse {
        StoredResponse {
            status: 200,
            status_text: String::new(),
            protocol: "http/1.1",
            headers: Vec::new(),
            request_time: Time::UNIX_EPOCH,
            response_time: Time::UNIX_EPOCH,
            vary: None,
            body: vec![Bytes::from(vec![b'x'; body])],
        }
    }

    #[test]
    fn the_bound_is_chromiums_for_a_big_machine() {
        let backend = MemoryBackend::default();
        assert_eq!(MAX_SIZE, 50 * 1024 * 1024);
        assert_eq!(backend.max_entry_size(), MAX_SIZE / 8);
    }

    #[test]
    fn an_entry_takes_its_key_and_response() {
        let mut backend = MemoryBackend::with_max_size(1000);
        assert!(backend.put("k", response(9)));
        assert_eq!(backend.size(), 10);
        assert_eq!(backend.get("k"), Some(&response(9)));
        assert_eq!(backend.get("other"), None);
        // A new one in its place.
        assert!(backend.put("k", response(19)));
        assert_eq!(backend.size(), 20);
        backend.remove("k");
        assert_eq!(backend.size(), 0);
        assert_eq!(backend.get("k"), None);
    }

    #[test]
    fn an_entry_bigger_than_an_eighth_isnt_kept() {
        let mut backend = MemoryBackend::with_max_size(800);
        assert!(backend.put("a", response(99)));
        // 100 is an eighth; one byte more isn't, and drops the one it was
        // to replace too.
        assert!(!backend.put("a", response(100)));
        assert_eq!(backend.get("a"), None);
        assert_eq!(backend.size(), 0);
    }

    #[test]
    fn past_the_bound_the_least_recently_used_go_to_90_percent() {
        let mut backend = MemoryBackend::with_max_size(1000);
        for key in ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"] {
            assert!(backend.put(key, response(99)));
        }
        assert_eq!(backend.size(), 1000);
        // Used, so not the oldest any more.
        assert!(backend.get("a").is_some());
        // 1100 is past 1000: down to 900, the oldest (`b`, `c`) going.
        assert!(backend.put("k", response(99)));
        assert_eq!(backend.size(), 900);
        assert!(backend.get("a").is_some());
        assert_eq!(backend.get("b"), None);
        assert_eq!(backend.get("c"), None);
        assert!(backend.get("d").is_some());
        assert!(backend.get("k").is_some());
    }

    #[test]
    fn clearing_drops_everything() {
        let mut backend = MemoryBackend::with_max_size(1000);
        assert!(backend.put("a", response(5)));
        assert!(backend.put("b", response(5)));
        backend.clear();
        assert_eq!(backend.size(), 0);
        assert_eq!(backend.get("a"), None);
        assert!(backend.put("c", response(5)));
        assert_eq!(backend.size(), 6);
    }
}
