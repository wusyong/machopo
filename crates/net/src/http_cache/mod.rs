//! The HTTP cache (Chromium's `net::HttpCache`): between a load's hop and
//! the transport, it answers from a stored response, validates one, or
//! sends the request on and stores what comes back.
//!
//! - [`headers`]: what a response's headers say to it
//!   (`HttpResponseHeaders`), freshness, age and validation;
//! - [`vary`]: which requests a stored response answers (`HttpVaryData`);
//! - [`request_headers`]: what a request's own headers say to it
//!   (`http_cache_util`);
//! - [`key`]: what a request's entry is kept under, partitioned by the
//!   top-level frame's and the frame's sites (`GenerateCacheKey`);
//! - [`entry`]: a stored response;
//! - [`memory`]: the entries in memory, bounded (`MemBackendImpl`): a
//!   profile in memory's;
//! - [`disk`] and [`sql_store`]: the entries in a SQLite database on a
//!   thread of its own, its index in memory (Chromium's SQL disk cache
//!   backend): a profile in a folder's.
//!
//! The rules follow Chromium's, not only RFC 9111, where the two differ: a
//! page sees what it sees in Chrome. Dates are read by the cookie date
//! parser, which agrees with Chromium's `Time::FromUTCString` on HTTP's
//! three date formats but ignores a time zone other than GMT.

pub(crate) mod disk;
pub(crate) mod entry;
pub(crate) mod headers;
pub(crate) mod key;
pub(crate) mod memory;
pub(crate) mod request_headers;
pub(crate) mod sql_store;
pub(crate) mod transaction;
pub(crate) mod vary;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bytes::Bytes;
use tokio::sync::{OnceCell, oneshot};

use crate::cookies::Time;
use disk::DiskBackend;
use entry::StoredResponse;
use memory::MemoryBackend;
use sql_store::{Loaded, SqlStore, SqlStoreThread};

/// The network context's HTTP cache. Clones share it: the loads are tasks
/// of the network service's one thread, which a lock lets share it (tokio's
/// tasks are `Send`; it is never held across an `await`, so never
/// contended), as the cookie jar is.
#[derive(Clone, Default)]
pub(crate) struct HttpCache {
    state: Arc<Mutex<State>>,
    /// Set once the store's index is in: what a load waits for first.
    loaded: Arc<OnceCell<()>>,
    /// The store's index, until the first load to wait takes it.
    loading: Arc<Mutex<Option<Loaded>>>,
}

#[derive(Default)]
struct State {
    backend: Backend,
    /// The keys a load is going to the network for, each with the loads
    /// waiting for it to be over: a later one waits, then looks again
    /// (Chromium's `ActiveEntry` lets one transaction write at a time).
    writing: HashMap<String, Vec<oneshot::Sender<()>>>,
    /// How many times it was cleared (or its database started over): a
    /// response being written then is dropped once it is all there
    /// (Chromium dooms the active entries too).
    clears: u64,
}

enum Backend {
    Memory(MemoryBackend),
    Disk(DiskBackend),
}

impl Default for Backend {
    fn default() -> Self {
        Self::Memory(MemoryBackend::default())
    }
}

impl State {
    fn cleared(&mut self) {
        self.clears = self.clears.saturating_add(1);
    }
}

impl HttpCache {
    /// One on disk: the `Cache` database of the cache folder `dir`, bounded
    /// at `max_size` bytes; and its thread's handle, which the network
    /// context keeps.
    pub(crate) fn on_disk(dir: &Path, max_size: u64) -> std::io::Result<(Self, SqlStoreThread)> {
        let state = Arc::new(Mutex::new(State::default()));
        // Weak: the store's thread is the state's, and outlives no handle.
        let razed = Arc::downgrade(&state);
        let (store, thread, loaded) = SqlStore::open(dir, move || {
            if let Some(state) = razed.upgrade() {
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                if let Backend::Disk(disk) = &mut state.backend {
                    disk.razed();
                }
                state.cleared();
            }
        })?;
        state.lock().unwrap_or_else(PoisonError::into_inner).backend =
            Backend::Disk(DiskBackend::new(store, max_size));
        let cache = Self {
            state,
            loaded: Arc::default(),
            loading: Arc::new(Mutex::new(Some(loaded))),
        };
        Ok((cache, thread))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits for the store's index to be in (at once in memory, or once
    /// it is): what a load does before it looks.
    pub(crate) async fn wait_for_load(&self) {
        self.loaded
            .get_or_init(|| async {
                let loading = self
                    .loading
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                // Taken once: the first to wait loads it, the others wait
                // for it.
                if let Some(loading) = loading {
                    // None if the store's thread is gone: nothing in.
                    let entries = loading.await.unwrap_or_default();
                    if let Backend::Disk(disk) = &mut self.state().backend {
                        disk.load(entries, Time::now());
                    }
                }
            })
            .await;
    }

    /// `key`'s for this load, until the slot goes; or, when another load
    /// has it, what tells once that one is over.
    pub(crate) fn begin(&self, key: &str) -> Result<Slot, oneshot::Receiver<()>> {
        let mut state = self.state();
        if let Some(waiting) = state.writing.get_mut(key) {
            let (tell, told) = oneshot::channel();
            waiting.push(tell);
            return Err(told);
        }
        state.writing.insert(key.to_owned(), Vec::new());
        Ok(Slot {
            cache: self.clone(),
            key: key.to_owned(),
            clears: state.clears,
        })
    }

    /// `key`'s stored response, now the most recently used.
    pub(crate) async fn get(&self, key: &str) -> Option<StoredResponse> {
        let read = match &mut self.state().backend {
            Backend::Memory(memory) => return memory.get(key).cloned(),
            Backend::Disk(disk) => disk.read(key, Time::now())?,
        };
        let response = read.await.ok().flatten();
        if response.is_none()
            && let Backend::Disk(disk) = &mut self.state().backend
        {
            // The store has none (its row gone, or unreadable).
            disk.forget(key);
        }
        response
    }

    /// `key`'s stored response has `response`'s head (a `304`'s), its body
    /// as it was.
    pub(crate) fn update(&self, key: &str, response: StoredResponse) {
        match &mut self.state().backend {
            Backend::Memory(memory) => {
                memory.put(key, response);
            }
            Backend::Disk(disk) => disk.update(key, response, Time::now()),
        }
    }

    /// Drops `key`'s entry (`DoomEntry`).
    pub(crate) fn remove(&self, key: &str) {
        match &mut self.state().backend {
            Backend::Memory(memory) => memory.remove(key),
            Backend::Disk(disk) => disk.remove(key),
        }
    }

    /// Drops every entry (`DoomAllEntries`), and those being written; then
    /// `done`, once they are gone from disk too.
    pub(crate) fn clear(&self, done: impl FnOnce() + Send + 'static) {
        let mut state = self.state();
        state.cleared();
        match &mut state.backend {
            Backend::Memory(memory) => {
                memory.clear();
                drop(state);
                done();
            }
            Backend::Disk(disk) => disk.clear(done),
        }
    }

    /// The most a stored response may take.
    pub(crate) fn max_entry_size(&self) -> u64 {
        match &self.state().backend {
            Backend::Memory(memory) => memory.max_entry_size(),
            Backend::Disk(disk) => disk.max_entry_size(),
        }
    }

    fn end(&self, key: &str) {
        let waiting = self.state().writing.remove(key).unwrap_or_default();
        for tell in waiting {
            let _ = tell.send(());
        }
    }

    /// Starts writing `head`, its body to come, as `key`'s; whether it is
    /// written as it comes (on disk), not kept whole until it is all there.
    fn begin_write(&self, key: &str, head: &StoredResponse) -> bool {
        match &self.state().backend {
            Backend::Memory(_) => false,
            Backend::Disk(disk) => {
                disk.begin(key, head);
                true
            }
        }
    }

    fn append(&self, key: &str, piece: Bytes) {
        if let Backend::Disk(disk) = &self.state().backend {
            disk.append(key, piece);
        }
    }

    fn abandon(&self, key: &str) {
        if let Backend::Disk(disk) = &self.state().backend {
            disk.abandon(key);
        }
    }

    /// `key`'s response is all there: kept (in memory, `response`; on disk,
    /// what was written of it, `size` bytes), unless the cache was cleared
    /// since `clears`.
    fn finish_write(&self, key: &str, response: StoredResponse, size: u64, clears: u64) {
        let mut state = self.state();
        if state.clears != clears {
            return;
        }
        match &mut state.backend {
            Backend::Memory(memory) => {
                memory.put(key, response);
            }
            Backend::Disk(disk) => disk.finish(key, size, Time::now()),
        }
    }

    /// The disk's entries, for tests.
    #[cfg(test)]
    fn with_disk<R>(&self, f: impl FnOnce(&mut DiskBackend) -> R) -> Option<R> {
        match &mut self.state().backend {
            Backend::Memory(_) => None,
            Backend::Disk(disk) => Some(f(disk)),
        }
    }
}

/// A load's hold on a key while it goes to the network: the loads after it
/// wait until it goes, then look again.
pub(crate) struct Slot {
    cache: HttpCache,
    key: String,
    /// The cache's clears as it was taken.
    clears: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.cache.end(&self.key);
    }
}

/// What keeps a response's body as it comes from the wire, and stores the
/// response once it is all there: in memory, its pieces kept until then;
/// on disk, written as they come. Dropped before that (the load failed or
/// was cancelled), nothing is stored (what was written goes). Either way,
/// its slot goes then.
pub(crate) struct Recorder {
    slot: Slot,
    /// Until it is kept (in memory, with the pieces so far).
    response: Option<StoredResponse>,
    size: u64,
    max_size: u64,
    /// Written to disk as it comes.
    on_disk: bool,
}

impl Recorder {
    /// For `response` (its body still to come), under `slot`'s key.
    pub(crate) fn new(slot: Slot, response: StoredResponse) -> Self {
        let max_size = slot.cache.max_entry_size();
        let size = response.size();
        let on_disk = slot.cache.begin_write(&slot.key, &response);
        Self {
            slot,
            response: Some(response),
            size,
            max_size,
            on_disk,
        }
    }

    /// Keeps `piece`; past the most an entry may take, it keeps nothing
    /// more, and stores nothing.
    pub(crate) fn push(&mut self, piece: &Bytes) {
        if self.size > self.max_size {
            return;
        }
        self.size = self
            .size
            .saturating_add(u64::try_from(piece.len()).unwrap_or(u64::MAX));
        if self.size > self.max_size {
            self.give_up();
        } else if self.on_disk {
            self.slot.cache.append(&self.slot.key, piece.clone());
        } else if let Some(response) = &mut self.response {
            response.body.push(piece.clone());
        }
    }

    /// The body is all there: stores the response, if it isn't too big.
    pub(crate) fn finish(mut self) {
        if let Some(response) = self.response.take()
            && self.size <= self.max_size
        {
            self.slot
                .cache
                .finish_write(&self.slot.key, response, self.size, self.slot.clears);
        }
    }

    /// What was written of it goes (on disk, at once, not once the load
    /// is over).
    fn give_up(&mut self) {
        if self.response.take().is_some() && self.on_disk {
            self.slot.cache.abandon(&self.slot.key);
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.give_up();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use sql_store::IndexEntry;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("machopo-http-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn on_disk(dir: &Path) -> (HttpCache, SqlStoreThread) {
        let (cache, thread) = HttpCache::on_disk(dir, 1024 * 1024 * 1024).unwrap();
        block_on(cache.wait_for_load());
        (cache, thread)
    }

    fn head() -> StoredResponse {
        StoredResponse {
            status: 200,
            status_text: "OK".to_owned(),
            protocol: "http/1.1",
            headers: vec![("etag".to_owned(), "\"v1\"".to_owned())],
            request_time: Time::UNIX_EPOCH,
            response_time: Time::UNIX_EPOCH,
            vary: None,
            body: Vec::new(),
        }
    }

    /// Writes `pieces` as `key`'s body through a recorder; `finish`: whether
    /// it is all there.
    fn record(cache: &HttpCache, key: &str, pieces: &[Bytes], finish: bool) {
        let slot = cache.begin(key).ok().unwrap();
        let mut recorder = Recorder::new(slot, head());
        for piece in pieces {
            recorder.push(piece);
        }
        if finish {
            recorder.finish();
        }
    }

    #[test]
    fn a_recorder_on_disk_writes_as_it_comes() {
        let dir = scratch("record");
        let (cache, thread) = on_disk(&dir);
        let pieces = [Bytes::from_static(b"ab"), Bytes::from_static(b"c")];
        record(&cache, "k", &pieces, true);
        let size = StoredResponse {
            body: pieces.to_vec(),
            ..head()
        }
        .size();
        assert_eq!(cache.with_disk(|disk| disk.size()), Some(size));
        assert_eq!(block_on(cache.get("k")).unwrap().body.concat(), b"abc");
        // Dropped before it is all there: none, and nothing left, not even
        // until the next start.
        record(&cache, "cut", &pieces, false);
        assert_eq!(cache.with_disk(|disk| disk.has("cut")), Some(false));
        drop((cache, thread));
        assert_eq!(sql_store::rows(&dir, "resources"), 1);
        let (cache, _thread) = on_disk(&dir);
        assert_eq!(block_on(cache.get("k")).unwrap().body.concat(), b"abc");
        assert_eq!(block_on(cache.get("cut")), None);
    }

    #[test]
    fn one_too_big_isnt_kept_on_disk() {
        let dir = scratch("too-big");
        let (cache, thread) = on_disk(&dir);
        // An eighth of 1 GiB is 128 MiB: in pieces of 1 MiB.
        let piece = Bytes::from(vec![0u8; 1024 * 1024]);
        let pieces = vec![piece; 129];
        record(&cache, "k", &pieces, true);
        assert_eq!(cache.with_disk(|disk| disk.has("k")), Some(false));
        drop((cache, thread));
        // What was written of it went as it was given up on.
        assert_eq!(sql_store::rows(&dir, "resources"), 0);
        assert_eq!(sql_store::rows(&dir, "blobs"), 0);
        let (cache, _thread) = on_disk(&dir);
        assert_eq!(block_on(cache.get("k")), None);
    }

    #[test]
    fn one_being_written_as_the_cache_clears_isnt_kept() {
        let dir = scratch("cleared");
        let (cache, _thread) = on_disk(&dir);
        let slot = cache.begin("k").ok().unwrap();
        let mut recorder = Recorder::new(slot, head());
        recorder.push(&Bytes::from_static(b"a"));
        let (done, cleared) = oneshot::channel();
        cache.clear(move || {
            let _ = done.send(());
        });
        block_on(cleared).unwrap();
        recorder.push(&Bytes::from_static(b"b"));
        recorder.finish();
        assert_eq!(cache.with_disk(|disk| disk.has("k")), Some(false));
        assert_eq!(block_on(cache.get("k")), None);
    }

    #[test]
    fn an_entry_the_store_hasnt_is_forgotten() {
        let dir = scratch("forgotten");
        let (cache, _thread) = on_disk(&dir);
        cache.with_disk(|disk| {
            disk.load(
                vec![IndexEntry {
                    key: "ghost".to_owned(),
                    size: 10,
                    last_used: Time::UNIX_EPOCH,
                }],
                Time::UNIX_EPOCH,
            );
        });
        assert_eq!(block_on(cache.get("ghost")), None);
        assert_eq!(cache.with_disk(|disk| disk.has("ghost")), Some(false));
        assert_eq!(cache.with_disk(|disk| disk.size()), Some(0));
        // A 304 for none updates none.
        cache.update("none", head());
        assert_eq!(cache.with_disk(|disk| disk.has("none")), Some(false));
    }

    #[test]
    fn removed_and_updated_on_disk_too() {
        let dir = scratch("removed");
        let (cache, thread) = on_disk(&dir);
        record(&cache, "gone", &[Bytes::from_static(b"a")], true);
        record(&cache, "big", &[Bytes::from_static(b"a")], true);
        cache.remove("gone");
        // A 304 whose headers make it more than an entry may take.
        let huge = StoredResponse {
            headers: vec![("x".to_owned(), "x".repeat(129 * 1024 * 1024))],
            ..head()
        };
        cache.update("big", huge);
        assert_eq!(cache.with_disk(|disk| disk.size()), Some(0));
        drop((cache, thread));
        let (cache, _thread) = on_disk(&dir);
        assert_eq!(block_on(cache.get("gone")), None);
        assert_eq!(block_on(cache.get("big")), None);
    }

    #[test]
    fn a_cache_in_memory_keeps_the_response_whole() {
        let cache = HttpCache::default();
        assert_eq!(cache.with_disk(|_| ()), None);
        record(&cache, "k", &[Bytes::from_static(b"ab")], true);
        assert_eq!(block_on(cache.get("k")).unwrap().body.concat(), b"ab");
    }
}
