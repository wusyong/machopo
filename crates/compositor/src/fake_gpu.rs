//! The compositor's shared images for tests, with no Viz: what it sent.

use std::sync::{Arc, Mutex};

use protocol::viz::{CompositorFrameSinkMsg, Mailbox};

use crate::SharedImages;

/// A compositor's [`SharedImages`] whose messages are kept, to see which
/// shared images it made and which it has destroyed.
pub(crate) struct FakeGpu {
    pub(crate) images: Arc<SharedImages>,
    sent: Arc<Mutex<Vec<CompositorFrameSinkMsg>>>,
}

impl Default for FakeGpu {
    fn default() -> Self {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let to = sent.clone();
        Self {
            images: SharedImages::new(move |msg| to.lock().unwrap().push(msg)),
            sent,
        }
    }
}

impl FakeGpu {
    /// The shared images made, in order: each one's mailbox and pixels.
    pub(crate) fn created(&self) -> Vec<(Mailbox, bytes::Bytes)> {
        let sent = self.sent.lock().unwrap();
        sent.iter()
            .filter_map(|msg| match msg {
                CompositorFrameSinkMsg::CreateSharedImage {
                    mailbox, pixels, ..
                } => Some((*mailbox, pixels.as_bytes()?.clone())),
                _ => None,
            })
            .collect()
    }

    /// How many were made.
    pub(crate) fn made(&self) -> usize {
        self.created().len()
    }

    pub(crate) fn destroyed(&self) -> Vec<Mailbox> {
        let sent = self.sent.lock().unwrap();
        sent.iter()
            .filter_map(|msg| match msg {
                CompositorFrameSinkMsg::DestroySharedImage { mailbox } => Some(*mailbox),
                _ => None,
            })
            .collect()
    }

    /// How many made and not destroyed.
    pub(crate) fn alive(&self) -> usize {
        self.made() - self.destroyed().len()
    }
}
