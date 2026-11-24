//! A profile's storage, which each of its tabs shares (Chromium's
//! `StoragePartition`): its network context (the cookies, the blob URLs)
//! and its `localStorage`.

use net::{NetworkContext, NetworkContextParams};

use crate::{DomStorageContext, Profile};

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
}
