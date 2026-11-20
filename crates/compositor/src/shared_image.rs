//! The compositor's side of shared images (Chromium's
//! `gpu::SharedImageInterfaceProxy` and `ClientSharedImage`): a tile's
//! pixels go to Viz in a message as soon as they are drawn, under a number
//! of the compositor's own, and the frames that show the tile name the
//! number alone.
//!
//! The pixels are copied once on their way to the GPU, by Viz: a tile is
//! drawn straight into a buffer of the pool here ([`PixelBuffer`]), the
//! message shares that buffer rather than copying it, and Viz hands it back
//! once the pixels are on the GPU, for the next tile. A buffer stands in
//! for a piece of a shared-memory region the browser gave the renderer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use base::AtomicSequenceNumber;
use bytes::Bytes;
use gfx::Size;
use protocol::viz::{CompositorFrameSinkMsg, Mailbox};

/// How many buffers the pool keeps for the next tiles: more than a frame
/// rasters at once come back to it, and the rest are let go.
const POOL_BUFFERS: usize = 64;

/// A buffer of the pool, the compositor's alone: a tile is drawn into it.
pub struct PixelBuffer(Arc<[u8]>);

impl PixelBuffer {
    pub fn pixels(&self) -> &[u8] {
        &self.0
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        Arc::get_mut(&mut self.0).expect("a buffer taken from the pool is held by no one else")
    }
}

/// The buffer as the bytes of a message: shared, not copied.
struct Lent(Arc<[u8]>);

impl AsRef<[u8]> for Lent {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Default)]
struct Pool {
    /// Buffers no one else holds.
    free: Vec<Arc<[u8]>>,
    /// The buffers Viz has, by the shared image each went with, until it
    /// says it holds one no more.
    lent: HashMap<Mailbox, Arc<[u8]>>,
    /// How many buffers were made, for tests: a pool that works makes few.
    made: usize,
}

/// A compositor's shared images: makes them, and keeps the pool their
/// pixels are drawn into. Shared by the threads that raster.
pub struct SharedImages {
    send: Box<dyn Fn(CompositorFrameSinkMsg) + Send + Sync>,
    mailboxes: AtomicSequenceNumber<u64>,
    pool: Mutex<Pool>,
}

impl SharedImages {
    /// `send` takes each message to Viz, in the order the frames go.
    pub fn new(send: impl Fn(CompositorFrameSinkMsg) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            send: Box::new(send),
            mailboxes: AtomicSequenceNumber::<u64>::starting_at(1),
            pool: Mutex::new(Pool::default()),
        })
    }

    fn pool(&self) -> std::sync::MutexGuard<'_, Pool> {
        self.pool.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A buffer of `len` bytes to draw a tile into: one of the pool's of
    /// that length, or a new one. What it holds is whatever was drawn
    /// there before.
    pub fn take_buffer(&self, len: usize) -> PixelBuffer {
        let mut pool = self.pool();
        let found = pool
            .free
            .iter()
            .position(|buffer| buffer.len() == len && Arc::strong_count(buffer) == 1);
        match found {
            Some(index) => PixelBuffer(pool.free.swap_remove(index)),
            None => {
                pool.made = pool.made.saturating_add(1);
                PixelBuffer(Arc::from(vec![0; len]))
            }
        }
    }

    /// A buffer not sent after all (its tile came out one color) is the
    /// pool's again.
    pub fn recycle(&self, buffer: PixelBuffer) {
        self.put(buffer.0);
    }

    fn put(&self, buffer: Arc<[u8]>) {
        let mut pool = self.pool();
        if pool.free.len() < POOL_BUFFERS {
            pool.free.push(buffer);
        }
    }

    /// The pixels in `buffer` as a shared image of `size`: told to Viz
    /// now, with the buffer itself. The buffer comes back to the pool when
    /// Viz says so ([`pixels_returned`](Self::pixels_returned)).
    pub fn create(self: &Arc<Self>, size: Size, buffer: PixelBuffer) -> ClientSharedImage {
        let mailbox = Mailbox(self.mailboxes.get_next());
        self.pool().lent.insert(mailbox, buffer.0.clone());
        self.send_create(mailbox, size, Bytes::from_owner(Lent(buffer.0)))
    }

    /// `pixels`, someone else's to keep (a canvas's snapshot), as a shared
    /// image of `size`: shared with Viz while it puts them on the GPU, not
    /// copied.
    pub fn create_from(self: &Arc<Self>, size: Size, pixels: Bytes) -> ClientSharedImage {
        let mailbox = Mailbox(self.mailboxes.get_next());
        self.send_create(mailbox, size, pixels)
    }

    fn send_create(
        self: &Arc<Self>,
        mailbox: Mailbox,
        size: Size,
        pixels: Bytes,
    ) -> ClientSharedImage {
        (self.send)(CompositorFrameSinkMsg::CreateSharedImage {
            mailbox,
            size,
            pixels,
        });
        ClientSharedImage(Arc::new(Owned {
            mailbox,
            images: self.clone(),
        }))
    }

    /// Viz holds the buffer that went with `mailbox` no more
    /// (`CompositorMsg::SharedImagePixelsReturned`): it is the pool's.
    pub fn pixels_returned(&self, mailbox: Mailbox) {
        let returned = self.pool().lent.remove(&mailbox);
        if let Some(buffer) = returned {
            self.put(buffer);
        }
    }

    /// How many buffers the pool has made.
    pub fn buffers_made(&self) -> usize {
        self.pool().made
    }
}

/// A shared image the compositor made, held by what shows it (a tile, a
/// canvas's layer): cloned, it is the same one, and Viz is told it is done
/// with (`DestroySharedImage`) when the last holder lets go.
#[derive(Clone)]
pub struct ClientSharedImage(Arc<Owned>);

struct Owned {
    mailbox: Mailbox,
    images: Arc<SharedImages>,
}

impl ClientSharedImage {
    pub fn mailbox(&self) -> Mailbox {
        self.0.mailbox
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // A buffer Viz never said it gave back isn't kept past its image.
        self.images.pool().lent.remove(&self.mailbox);
        (self.images.send)(CompositorFrameSinkMsg::DestroySharedImage {
            mailbox: self.mailbox,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn images() -> (Arc<SharedImages>, Arc<Mutex<Vec<CompositorFrameSinkMsg>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let to = sent.clone();
        (
            SharedImages::new(move |msg| to.lock().unwrap().push(msg)),
            sent,
        )
    }

    /// A tile drawn into a buffer of the pool goes to Viz as that buffer,
    /// not a copy of it; the buffer is drawn into again once Viz has
    /// handed it back, and not before. The image is destroyed when its
    /// last holder lets go.
    #[test]
    fn a_tiles_pixels_are_the_pools_buffer_and_come_back_to_it() {
        let (images, sent) = images();
        let mut buffer = images.take_buffer(8);
        buffer.pixels_mut().copy_from_slice(&[7; 8]);
        let drawn_at = buffer.pixels().as_ptr();
        let image = images.create(Size::new(2, 1), buffer);
        let held = image.clone();
        let sent_pixels = match sent.lock().unwrap().pop() {
            Some(CompositorFrameSinkMsg::CreateSharedImage {
                mailbox,
                size,
                pixels,
            }) => {
                assert_eq!((mailbox, size), (image.mailbox(), Size::new(2, 1)));
                pixels
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(sent_pixels.as_ptr(), drawn_at, "the buffer itself");
        assert_eq!(&sent_pixels[..], [7; 8]);

        // Viz still has it: the next tile gets another.
        let other = images.take_buffer(8);
        assert_ne!(other.pixels().as_ptr(), drawn_at);
        assert_eq!(images.buffers_made(), 2);
        // Told it came back while Viz still holds it (a Viz that lies):
        // not drawn into all the same, with no other buffer to take.
        images.pixels_returned(image.mailbox());
        let again = images.take_buffer(8);
        assert_ne!(again.pixels().as_ptr(), drawn_at);
        assert_eq!(images.buffers_made(), 3);
        drop(sent_pixels);
        let mut back = images.take_buffer(8);
        assert_eq!(back.pixels().as_ptr(), drawn_at);
        back.pixels_mut()[0] = 1;
        assert_eq!(images.buffers_made(), 3);
        images.recycle(other);
        images.recycle(again);
        // A buffer of another length isn't one of these.
        assert_eq!(images.take_buffer(4).pixels().len(), 4);
        assert_eq!(images.buffers_made(), 4);

        drop(image);
        assert!(sent.lock().unwrap().is_empty(), "a holder is left");
        let mailbox = held.mailbox();
        drop(held);
        assert!(matches!(
            sent.lock().unwrap()[..],
            [CompositorFrameSinkMsg::DestroySharedImage { mailbox: destroyed }] if destroyed == mailbox
        ));
    }

    /// Pixels kept elsewhere go to Viz shared too, and into no pool.
    #[test]
    fn pixels_of_someone_elses_are_shared_not_pooled() {
        let (images, sent) = images();
        let pixels = Bytes::from_static(&[1, 2, 3, 4]);
        let image = images.create_from(Size::new(1, 1), pixels.clone());
        match &sent.lock().unwrap()[0] {
            CompositorFrameSinkMsg::CreateSharedImage { pixels: sent, .. } => {
                assert_eq!(sent.as_ptr(), pixels.as_ptr());
            }
            other => panic!("{other:?}"),
        }
        images.pixels_returned(image.mailbox());
        assert_eq!(images.buffers_made(), 0);
        assert_ne!(images.take_buffer(4).pixels().as_ptr(), pixels.as_ptr());
    }
}
