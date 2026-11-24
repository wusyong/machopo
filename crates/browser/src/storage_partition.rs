//! A profile's storage, which each of its tabs shares (Chromium's
//! `StoragePartition`): its network context (the cookies, the blob URLs)
//! and its `localStorage`.

use std::time::Instant;

use net::{NetworkContext, NetworkContextParams};

use crate::{DomStorageContext, Profile, shutdown};

/// The HTTP cache's folder in the profile's cache folder (Chromium's
/// `chrome::kCacheDirname`).
const CACHE_DIRNAME: &str = "Cache";

#[derive(Clone)]
pub struct StoragePartition {
    network: NetworkContext,
    dom_storage: DomStorageContext,
}

impl StoragePartition {
    /// `profile`'s: its network context started with `network`'s params
    /// (`StoragePartitionImpl::InitNetworkContext`), its cookies kept in
    /// the profile's folder if it has one, its HTTP cache in the profile's
    /// cache folder (`ProfileImpl`'s `GetUserCacheDirectory`).
    pub fn new(profile: &Profile, network: NetworkContextParams) -> std::io::Result<Self> {
        Ok(Self {
            network: NetworkContext::start(NetworkContextParams {
                data_directory: profile.dir().map(std::path::Path::to_path_buf),
                http_cache_directory: profile.cache_dir().map(|dir| dir.join(CACHE_DIRNAME)),
                ..network
            })?,
            dom_storage: DomStorageContext::new(profile)?,
        })
    }

    pub fn network(&self) -> &NetworkContext {
        &self.network
    }

    pub fn dom_storage(&self) -> &DomStorageContext {
        &self.dom_storage
    }

    /// Closes the profile's databases as machopo ends, whichever handles
    /// are still about (each tab's, the IO thread's): what they were told
    /// is written, all at once, each waited for until [`PROFILE_LIMIT`] has
    /// passed; one that hasn't closed by then is let go, with a line
    /// (`shutdown::gave_up`), and its last writes may be lost. Called once
    /// the renderers have ended: what is told after is not written.
    /// (Chromium's profile is written as its `ProfileImpl` and services go
    /// in `ShutdownThreadsAndCleanUp`, with no limit of their own; its
    /// `ShutdownWatcherHelper` crashes the browser if that takes very long.)
    ///
    /// [`PROFILE_LIMIT`]: crate::shutdown::PROFILE_LIMIT
    pub fn close(&self) {
        let start = Instant::now();
        let deadline = start + shutdown::PROFILE_LIMIT;
        self.dom_storage.close();
        self.network.close();
        let mut late = self.network.closed_by(deadline);
        if !self.dom_storage.closed_by(deadline) {
            late.push("the localStorage database");
        }
        for what in late {
            shutdown::gave_up(what, "being written", start.elapsed());
        }
    }
}
