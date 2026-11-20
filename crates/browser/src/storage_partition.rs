//! A profile's storage, which each of its tabs shares (Chromium's
//! `StoragePartition`): its network context (the cookies, the blob URLs)
//! and its `localStorage`.

use net::{NetworkContext, NetworkContextParams};

use crate::{DomStorageContext, Profile};

#[derive(Clone)]
pub struct StoragePartition {
    network: NetworkContext,
    dom_storage: DomStorageContext,
}

impl StoragePartition {
    /// `profile`'s: its network context started with `network`'s params
    /// (`StoragePartitionImpl::InitNetworkContext`), its cookies kept in
    /// the profile's folder if it has one.
    pub fn new(profile: &Profile, network: NetworkContextParams) -> std::io::Result<Self> {
        Ok(Self {
            network: NetworkContext::start(NetworkContextParams {
                data_directory: profile.dir().map(std::path::Path::to_path_buf),
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
