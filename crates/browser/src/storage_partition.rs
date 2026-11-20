//! A profile's storage, which each of its tabs shares (Chromium's
//! `StoragePartition`): its network context (the cookies, the blob URLs)
//! and its `localStorage`.

use net::NetworkContext;

use crate::{DomStorageContext, Profile};

#[derive(Clone)]
pub struct StoragePartition {
    network: NetworkContext,
    dom_storage: DomStorageContext,
}

impl StoragePartition {
    /// `profile`'s, its requests on `network`.
    pub fn new(profile: &Profile, network: NetworkContext) -> std::io::Result<Self> {
        Ok(Self {
            network,
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
