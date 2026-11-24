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
//! - [`memory`]: the entries in memory, bounded (`MemBackendImpl`).
//!
//! The rules follow Chromium's, not only RFC 9111, where the two differ: a
//! page sees what it sees in Chrome. Dates are read by the cookie date
//! parser, which agrees with Chromium's `Time::FromUTCString` on HTTP's
//! three date formats but ignores a time zone other than GMT.

pub(crate) mod entry;
pub(crate) mod headers;
pub(crate) mod key;
pub(crate) mod memory;
pub(crate) mod request_headers;
pub(crate) mod vary;
pub(crate) mod transaction;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bytes::Bytes;
use tokio::sync::oneshot;

use entry::StoredResponse;
use memory::MemoryBackend;

/// The network context's HTTP cache. Clones share it: the loads are tasks
/// of the network service's one thread, which a lock lets share it (tokio's
/// tasks are `Send`; it is never held across an `await`, so never
/// contended), as the cookie jar is.
#[derive(Clone, Default)]
pub(crate) struct HttpCache {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    backend: MemoryBackend,
    /// The keys a load is going to the network for, each with the loads
    /// waiting for it to be over: a later one waits, then looks again
    /// (Chromium's `ActiveEntry` lets one transaction write at a time).
    writing: HashMap<String, Vec<oneshot::Sender<()>>>,
    /// How many times it was cleared: a response being written when it was
    /// is dropped once it is all there (Chromium dooms the active entries
    /// too).
    clears: u64,
}

impl HttpCache {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
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
    pub(crate) fn get(&self, key: &str) -> Option<StoredResponse> {
        self.state().backend.get(key).cloned()
    }

    pub(crate) fn put(&self, key: &str, response: StoredResponse) {
        self.state().backend.put(key, response);
    }

    /// Drops `key`'s entry (`DoomEntry`).
    pub(crate) fn remove(&self, key: &str) {
        self.state().backend.remove(key);
    }

    /// Drops every entry (`DoomAllEntries`), and those being written.
    pub(crate) fn clear(&self) {
        let mut state = self.state();
        state.backend.clear();
        state.clears = state.clears.saturating_add(1);
    }

    /// Keeps `response` as `key`'s, unless the cache was cleared since
    /// `clears`.
    fn put_unless_cleared(&self, key: &str, response: StoredResponse, clears: u64) {
        let mut state = self.state();
        if state.clears == clears {
            state.backend.put(key, response);
        }
    }

    /// The most a stored response may take.
    pub(crate) fn max_entry_size(&self) -> u64 {
        self.state().backend.max_entry_size()
    }

    fn end(&self, key: &str) {
        let waiting = self.state().writing.remove(key).unwrap_or_default();
        for tell in waiting {
            let _ = tell.send(());
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
/// response once it is all there; dropped before that (the load failed or
/// was cancelled), nothing is stored. Either way, its slot goes then.
pub(crate) struct Recorder {
    slot: Slot,
    response: StoredResponse,
    size: u64,
    max_size: u64,
}

impl Recorder {
    /// For `response` (its body still to come), under `slot`'s key.
    pub(crate) fn new(slot: Slot, response: StoredResponse) -> Self {
        let max_size = slot.cache.max_entry_size();
        let size = response.size();
        Self {
            slot,
            response,
            size,
            max_size,
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
            self.response.body.clear();
        } else {
            self.response.body.push(piece.clone());
        }
    }

    /// The body is all there: stores the response, if it isn't too big.
    pub(crate) fn finish(self) {
        if self.size <= self.max_size {
            self.slot
                .cache
                .put_unless_cleared(&self.slot.key, self.response, self.slot.clears);
        }
    }
}
