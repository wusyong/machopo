//! A `SharedImageInterface` for tests, with no GPU.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use base::AtomicSequenceNumber;
use gfx::Size;
use protocol::viz::{ContextLostObserver, Mailbox, SharedImage, SharedImageInterface};

/// Makes shared images that are the pixels they were made from, and
/// remembers them to see when they are let go; or, when `full`, none.
/// `lose` plays Viz losing the GPU.
#[derive(Default)]
pub(crate) struct FakeGpu {
    pub(crate) full: AtomicBool,
    mailboxes: AtomicSequenceNumber<u64>,
    pub(crate) made: Mutex<Vec<Weak<Vec<u8>>>>,
    observers: Mutex<Vec<Weak<dyn ContextLostObserver>>>,
}

impl FakeGpu {
    pub(crate) fn alive(&self) -> usize {
        let made = self.made.lock().unwrap();
        made.iter().filter(|image| image.strong_count() > 0).count()
    }

    /// Tells the observers still alive the GPU was lost; how many.
    pub(crate) fn lose(&self) -> usize {
        let observers = self.observers.lock().unwrap();
        let alive: Vec<_> = observers.iter().filter_map(Weak::upgrade).collect();
        for observer in &alive {
            observer.on_context_lost();
        }
        alive.len()
    }
}

impl SharedImageInterface for FakeGpu {
    fn create_shared_image(&self, _size: Size, pixels: &[u8]) -> Option<SharedImage> {
        if self.full.load(Ordering::Relaxed) {
            return None;
        }
        let backing = Arc::new(pixels.to_vec());
        self.made.lock().unwrap().push(Arc::downgrade(&backing));
        let mailbox = Mailbox(self.mailboxes.get_next());
        Some(SharedImage::new(mailbox, backing))
    }

    fn add_context_lost_observer(&self, observer: Weak<dyn ContextLostObserver>) {
        self.observers.lock().unwrap().push(observer);
    }
}
